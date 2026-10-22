# دليل الربط: Supabase (Auth + Edge Functions)

يوثّق هذا الدليل كيفية ربط التطبيقات التي تعتمد منصة **Supabase** مع خادم FCM المصغّر.
كُتب هذا الدليل استناداً لما يدعمه كود الأداة فعلياً دون افتراضات غير مدعومة.

---

## 1. متى تستعمل هذا الإعداد

- إذا كان تطبيقك يستخدم **Supabase Auth** لإدارة المستخدمين وتسجيل الدخول.
- إذا كان لديك قاعدة بيانات PostgreSQL على Supabase، وتريد إرسال إشعارات مبنية على قوالب آمنة ومتحقق منها.

---

## 2. إعداد قسم الهوية (`identity`)

يدعم خادم FCM المصغّر التحقق من توكنات JWT الصادرة من Supabase Auth بطريقتين:

### الخيار أ: التحقق السريع عبر السر المشترك (HS256)
وهي الطريقة الأكثر شيوعاً في Supabase. انسخ `JWT Secret` من لوحة تحكم Supabase
(من إعدادات المشروع `Project Settings -> API -> JWT Secret`) وضعه في متغير بيئة في خادمك
(مثلاً `SUPABASE_JWT_SECRET`):

```json
"identity": {
  "provider": {
    "type": "jwt",
    "algorithms": ["HS256"],
    "hs256_secret_env": "SUPABASE_JWT_SECRET",
    "issuer": "https://<PROJECT_REF>.supabase.co/auth/v1",
    "audience": "authenticated",
    "uid_claim": "sub",
    "name_claim": "user_metadata.name",
    "max_age_seconds": 3600
  },
  "role": {
    "source": "claim",
    "claim": "app_metadata.role"
  },
  "admin_roles": ["admin"]
}
```

### الخيار ب: التحقق غير المتماثل عبر JWKS (RS256/ES256)
إذا كان مشروعك في Supabase يتيح مفاتيح غير متماثلة:
```json
"provider": {
  "type": "jwt",
  "algorithms": ["RS256"],
  "jwks_url": "https://<PROJECT_REF>.supabase.co/auth/v1/.well-known/jwks.json",
  "issuer": "https://<PROJECT_REF>.supabase.co/auth/v1",
  "audience": "authenticated",
  "uid_claim": "sub",
  "max_age_seconds": 3600
}
```

---

## 3. إعداد مصدر البيانات (`data_source`) والحدود البرمجية

> [!IMPORTANT]
> **حدود برمجية واضحة (ما لا يدعمه الكود والبديل المعتمد):**
> لا يدعم كود الأداة الاتصال المباشر بواجهات PostgREST التابعة لـ Supabase، لأن خادم FCM
> يتطلب أن يكون مصدر البيانات إما Cloud Firestore أو **HTTP Resolver موقّعاً بـ HMAC-SHA256**.
> واجهات PostgREST لا تستقبل ولا تتحقق من ترويسات `X-FCM-Signature`.
>
> **الحل البديل المدعوم برمجياً:**
> كتابة **Supabase Edge Function** بسيطة (أو سيرفر وسيط صغير) تستقبل طلبات الأداة،
> وتتحقق من توقيع HMAC، ثم تقرأ من PostgreSQL عبر Supabase JS Client وتعيد النتيجة.

### إعداد قسم `data_source` في الداشبورد:
```json
"data_source": {
  "type": "http",
  "base_url": "https://<PROJECT_REF>.supabase.co/functions/v1/fcm-resolver",
  "secret_env": "SUPABASE_RESOLVER_SECRET",
  "timeout_ms": 3000
}
```

### مثال على Supabase Edge Function (`fcm-resolver/index.ts`):
```typescript
import { serve } from "https://deno.land/std@0.168.0/http/server.ts";
import { createClient } from "https://esm.sh/@supabase/supabase-js@2";

const secret = Deno.env.get("SUPABASE_RESOLVER_SECRET") ?? "";
const supabase = createClient(
  Deno.env.get("SUPABASE_URL") ?? "",
  Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? ""
);

serve(async (req) => {
  const url = new URL(req.url);
  const timestamp = req.headers.get("x-fcm-timestamp") ?? "";
  const signature = req.headers.get("x-fcm-signature") ?? "";

  // 1. فحص التوقيع HMAC
  const canonical = `GET\n${url.pathname}\n${url.search.slice(1)}\n${timestamp}`;
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"]
  );
  const sigBuf = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(canonical));
  const expectedSig = Array.from(new Uint8Array(sigBuf))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");

  if (expectedSig !== signature || Math.abs(Date.now() / 1000 - parseInt(timestamp)) > 60) {
    return new Response(JSON.stringify({ error: "unauthorized" }), { status: 401 });
  }

  // 2. معالجة قراءة المستند /doc
  if (url.pathname.endsWith("/doc")) {
    const path = url.searchParams.get("path") ?? "";
    const [table, id] = path.split("/");
    const { data, error } = await supabase.from(table).select("*").eq("id", id).maybeSingle();
    if (error || !data) return new Response(JSON.stringify({ error: "not found" }), { status: 404 });
    return new Response(JSON.stringify(data), { status: 200, headers: { "Content-Type": "application/json" } });
  }

  // 3. معالجة أعضاء الدور /members
  if (url.pathname.endsWith("/members")) {
    const col = url.searchParams.get("collection") ?? "profiles";
    const field = url.searchParams.get("field") ?? "role";
    const value = url.searchParams.get("value") ?? "";
    const limit = parseInt(url.searchParams.get("limit") ?? "50");
    const { data } = await supabase.from(col).select("id").eq(field, value).limit(limit);
    const uids = (data ?? []).map((row: any) => row.id);
    return new Response(JSON.stringify({ uids }), { status: 200, headers: { "Content-Type": "application/json" } });
  }

  return new Response("Not Found", { status: 404 });
});
```

---

## 4. تسجيل الجهاز من Flutter (Dart)

باستخدام حزمة `supabase_flutter`:

```dart
import 'dart:convert';
import 'dart:io';
import 'package:firebase_messaging/firebase_messaging.dart';
import 'package:http/http.dart' as http;
import 'package:supabase_flutter/supabase_flutter.dart';

class SupabaseFcmRegistrar {
  static const String serverBaseUrl = 'https://fcm.example.com';
  static const String appName = 'supabase_app';

  /// تسجيل جهاز المستخدم بتوكن جلسة Supabase
  static Future<void> registerDevice() async {
    final session = Supabase.instance.client.auth.currentSession;
    if (session == null) return;

    final jwt = session.accessToken;
    final fcmToken = await FirebaseMessaging.instance.getToken();
    if (fcmToken == null) return;

    final url = Uri.parse('$serverBaseUrl/api/v1/devices/register');
    final response = await http.post(
      url,
      headers: {
        'Content-Type': 'application/json',
        'Authorization': 'Bearer $jwt',
      },
      body: jsonEncode({
        'app': appName,
        'token': fcmToken,
        'platform': Platform.isAndroid ? 'android' : (Platform.isIOS ? 'ios' : 'web'),
      }),
    );

    if (response.statusCode != 200) {
      throw Exception('فشل تسجيل الجهاز: ${response.statusCode}');
    }
  }
}
```

---

## 5. إرسال قالب إشعار (`POST /api/v1/send/template`)

### 5.1 من التطبيق بتوكن مستخدم Supabase
```bash
curl -s -X POST https://fcm.example.com/api/v1/send/template \
  -H "Authorization: Bearer <SUPABASE_ACCESS_TOKEN>" \
  -H "Content-Type: application/json" \
  -d '{
    "app": "supabase_app",
    "template": "order_new",
    "params": {
      "order_id": "987"
    }
  }'
```

### 5.2 من الباك إند أو Edge Function بمفتاح التطبيق (`fcm_<app>_...`)
```python
import requests

APP_KEY = os.environ.get("FCM_APP_KEY", "fcm_supabase_app_<43-char-random>")
FCM_SERVER = "https://fcm.example.com"

response = requests.post(
    f"{FCM_SERVER}/api/v1/send/template",
    headers={
        "Authorization": f"Bearer {APP_KEY}",
        "Content-Type": "application/json",
    },
    json={
        "app": "supabase_app",
        "template": "admin_alert",
        "params": {
            "title": "مستخدم جديد",
            "body": "انضم مستخدم جديد للمنصة عبر Supabase Auth",
            "route": "/users/987"
        }
    },
    timeout=5
)

print(response.status_code, response.json())
```

---

## 6. الأخطاء الشائعة وحلولها

| الرمز | السبب | الحل |
|---|---|---|
| `401 Unauthorized` | انتهاء صلاحية توكن Supabase أو خطأ في سر HS256 المشترك. | تجديد التوكن عبر `supabase.auth.refreshSession()` والتحقق من قيمة السر. |
| `403 Forbidden` | فشل التحقق في شرط `require` من وثيقة PostgreSQL. | التأكد من أن حقل `buyer_id` أو `user_id` في جدول قاعدة البيانات يطابق معرّف المستخدم `sub`. |
| `503 Service Unavailable` | توقف Supabase Edge Function أو استغراقها أكثر من 3000ms. | مراجعة سجلات Edge Function في Supabase للتأكد من خلوها من الأخطاء البرمجية. |

---

## 7. فحص الاتصال من لوحة التحكم

1. **فحص الهوية:** أدخل توكن `access_token` الخاص بمستخدم Supabase للتحقق من قراءة `sub` والأدوار.
2. **فحص مصدر البيانات:** أدخل مساراً مثل `orders/987` للتأكد من استجابة Edge Function بالبنية المطلوبة.

---

## 8. تحقق منه

- **اختبار معالجة claims النقطية (`app_metadata.role`):** اختبار `identity::jwt::tests::jwt_nested_uid_claim` في `src/identity/jwt.rs`.
- **اختبار رفض خلط الخوارزميات (تأمين HS256 ضد RS256):** اختبارات `identity::jwt::tests::jwt_hs256_token_against_rs256_provider_rejected` في `src/identity/jwt.rs:410`.
- **اختبار التحقق من توقيع HMAC لمصدر HTTP:** اختبار `datasource::http::tests::http_doc_signature_verified` في `src/datasource/http.rs`.

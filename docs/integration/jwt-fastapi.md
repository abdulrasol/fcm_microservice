# دليل الربط: JWT + FastAPI (Resolver API)

يوثّق هذا الدليل ربط خادم FCM المصغّر مع أي تطبيق أو خادم خلفي مستقل (مثل Python FastAPI،
أو Django، أو Go، أو Node.js) باستخدام توكنات هوية JWT قياسية وعقد **HTTP Resolver**
لقراءة الوثائق والأدوار.

---

## 1. متى تستعمل هذا الإعداد

- إذا كان تطبيقك لا يستخدم Firebase للمصادقة أو تخزين البيانات، بل يمتلك باك إند خاصاً يصدر توكنات JWT (باستخدام سر HS256 أو مفاتيح RS256/ES256 عبر JWKS).
- إذا كانت قاعدة بياناتك علائقية (مثل PostgreSQL أو MySQL أو SQLite) وترغب بإبقائها خلف واجهة الباك إند البرمجية دون فتحها مباشرة للخادم.
- إذا أردت فصل منطق الصلاحيات وجلب البيانات بحيث يسأل خادم FCM خادمك الخلفي عبر عقد HTTP Resolver الموقَّع بـ HMAC.

---

## 2. إعداد قسم الهوية (`identity`)

في لوحة التحكم (`/apps/:app/settings` تحت تبويب "الهوية"):

```json
"identity": {
  "provider": {
    "type": "jwt",
    "algorithms": ["HS256"],
    "hs256_secret_env": "CLIENT_JWT_SECRET",
    "issuer": "https://api.example.com",
    "audience": "my-mobile-app",
    "uid_claim": "sub",
    "name_claim": "name",
    "max_age_seconds": 3600
  },
  "role": {
    "source": "claim",
    "claim": "role"
  },
  "admin_roles": ["admin"]
}
```

### تفاصيل الحقول:
- `type`: ثابت `jwt`.
- `algorithms`: قائمة الخوارزميات المعتمدة (`["HS256"]` أو `["RS256"]`). يمنع الخادم خلط الخوارزميات غير المتماثلة بالمتماثلة، ويرفض دائماً `none`.
- `hs256_secret_env`: اسم متغير البيئة في `.env` الذي يحفظ المفتاح السري (يجب ألا يقل عن 32 بايت). لا يُخزَّن السر في قاعدة بيانات الخادم.
- `issuer` و`audience`: التحقق الصارم من جهة الإصدار والجمهور المستهدف للتطابق التام مع ما يصدره الباك إند.
- `uid_claim`: مسار claim الذي يحمل معرّف المستخدم (افتراضياً `sub`). يدعم المسارات النقطية للمطالبات المتداخلة.
- `max_age_seconds`: أقصى عمر مسموح به للتوكن منذ وقت إصداره (`iat`).

---

## 3. إعداد مصدر البيانات (`data_source`) وعقد Resolver API

يطلب خادم FCM قراءة الوثائق من خادمك الخلفي عبر مسارين موقّعين بتوقيع رقمي HMAC-SHA256:

```json
"data_source": {
  "type": "http",
  "base_url": "http://127.0.0.1:8099/internal/fcm",
  "secret_env": "CLIENT_RESOLVER_SECRET",
  "timeout_ms": 3000
}
```

### عقد Resolver API (العقد الثابت):
1. **قراءة وثيقة (`GET /doc`):**
   - الطلب: `GET {base_url}/doc?path=<percent-encoded path>`
   - الاستجابة: `200 OK` مع كائن JSON للوثيقة، أو `404 Not Found` إذا لم تكن موجودة.
2. **قراءة أعضاء دور (`GET /members`):**
   - الطلب: `GET {base_url}/members?collection=<c>&field=<f>&value=<v>&limit=<n>`
   - الاستجابة: `200 OK` مع `{"uids": ["u1", "u2"]}`.

### التوقيع والتحقق الأمني (بلا Bearer):
كل طلب يرسله خادم FCM يحتوي على ترويستين:
- `X-FCM-Timestamp`: وقت الإرسال بالثواني (Unix Epoch).
- `X-FCM-Signature`: توقيع hex صغير لـ `HMAC-SHA256(secret, canonical)` حيث:
  ```text
  canonical = "GET" + "\n" + <مسار URL كما أُرسل> + "\n" + <نص الاستعلام الخام> + "\n" + <timestamp>
  ```
يرفض الباك إند أي طلب يختلف توقيته عن ساعة السيرفر بأكثر من 60 ثانية، أو لا يتطابق توقيعه.

المثال الجاهز متوفر في `examples/resolver_fastapi.py`.

---

## 4. تسجيل الجهاز من Flutter (Dart)

```dart
import 'dart:convert';
import 'dart:io';
import 'package:firebase_messaging/firebase_messaging.dart';
import 'package:http/http.dart' as http;

class FcmDeviceRegistrar {
  static const String serverBaseUrl = 'https://fcm.example.com';
  static const String appName = 'fastapi_app';

  /// تسجيل جهاز المستخدم بتوكن JWT الصادر من FastAPI
  static Future<void> registerDevice(String userJwtToken) async {
    final fcmToken = await FirebaseMessaging.instance.getToken();
    if (fcmToken == null) return;

    final url = Uri.parse('$serverBaseUrl/api/v1/devices/register');
    final response = await http.post(
      url,
      headers: {
        'Content-Type': 'application/json',
        'Authorization': 'Bearer $userJwtToken',
      },
      body: jsonEncode({
        'app': appName,
        'token': fcmToken,
        'platform': Platform.isAndroid ? 'android' : (Platform.isIOS ? 'ios' : 'web'),
      }),
    );

    if (response.statusCode != 200) {
      throw Exception('فشل تسجيل الجهاز: ${response.statusCode} - ${response.body}');
    }
  }
}
```

---

## 5. إرسال قالب إشعار (`POST /api/v1/send/template`)

### 5.1 من التطبيق بتوكن المستخدم JWT
```bash
curl -s -X POST https://fcm.example.com/api/v1/send/template \
  -H "Authorization: Bearer <USER_JWT_TOKEN>" \
  -H "Content-Type: application/json" \
  -d '{
    "app": "fastapi_app",
    "template": "order_new",
    "params": {
      "order_id": "abc123"
    }
  }'
```

### 5.2 من خادم FastAPI بمفتاح التطبيق (`fcm_<app>_...`)
```python
import requests

APP_KEY = os.environ.get("FCM_APP_KEY", "fcm_fastapi_app_<43-char-random>")
FCM_SERVER = "https://fcm.example.com"

response = requests.post(
    f"{FCM_SERVER}/api/v1/send/template",
    headers={
        "Authorization": f"Bearer {APP_KEY}",
        "Content-Type": "application/json",
    },
    json={
        "app": "fastapi_app",
        "template": "admin_alert",
        "params": {
            "title": "طلب جديد",
            "body": "تم استلام طلب جديد في المنظومة",
            "route": "/orders/abc123"
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
| `401 Unauthorized` | توكن JWT منتهٍ أو توقيعه لا يطابق السر المحدد في `hs256_secret_env`. | مراجعة السر وتأكد من مطابقة `iss` و`aud`. |
| `403 Forbidden` | دور المستخدم غير مخول أو فشل التحقق في شرط `require` من المستند. | التأكد من أن المستخدم يملك الوثيقة المحددة في الباك إند. |
| `503 Service Unavailable` | تعذر اتصال خادم FCM بخادم Resolver API أو استغرق الطلب وقتاً أطول من `timeout_ms`. | التأكد من تشغيل خادم FastAPI ومطابقة ترويسات التوقيع HMAC. |

---

## 7. فحص الاتصال من لوحة التحكم

1. **فحص مزود الهوية (`identity/test`):** أدخل توكن JWT مولَّد بنفس السر. سيعرض الخادم حالة التوكن والدور المستخرج وأسماء المطالبات.
2. **فحص مصدر البيانات (`data_source/test`):** أدخل مسار مستند تجريبي (مثل `store_orders/abc123`). سيقوم الخادم بإرسال طلب موقع لـ FastAPI وعرض بنية الحقول الناتجة.

---

## 8. تحقق منه

- **فحص اختبار التوقيع الذاتي:** `python3 examples/resolver_fastapi.py --selftest` (يطبع OK).
- **اختبار مصدر HTTP في Rust:** اختبارات `datasource::http::tests::http_doc_*` في `src/datasource/http.rs`.
- **اختبار مزود JWT في Rust:** اختبارات `identity::jwt::tests::jwt_hs256_*` في `src/identity/jwt.rs`.
- **التكامل الشامل:** اختبار `tests::template_works_with_jwt_identity_and_http_source` في `src/main.rs:2750`.

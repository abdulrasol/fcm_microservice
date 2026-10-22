# دليل الربط: Firebase (Auth + Firestore)

يوثّق هذا الدليل كيفية ربط التطبيقات التي تعتمد منظومة Google Firebase
(Firebase Authentication وCloud Firestore) مع خادم FCM المصغّر، كما في تطبيق "واط".

---

## 1. متى تستعمل هذا الإعداد

- إذا كان تطبيقك (Flutter أو Web أو React Native) يستعمل **Firebase Auth** للمصادقة وإدارة المستخدمين.
- إذا كانت وثائق وسجلات التطبيق مخزنة في **Cloud Firestore**.
- إذا كنت ترغب في إدارة الإرسال الآمن للأجهزة عبر توكنات مستخدمي Firebase مباشرة، دون الحاجة لباك إند وسيط لمعظم الإشعارات التفاعلية.

---

## 2. إعداد قسم الهوية (`identity`)

في لوحة التحكم (`/apps/:app/settings` تحت تبويب "الهوية"):

```json
"identity": {
  "provider": {
    "type": "firebase"
  },
  "role": {
    "source": "data_source",
    "doc": "users/{uid}",
    "field": "role",
    "cache_seconds": 60
  },
  "admin_roles": ["admin"],
  "role_members": {
    "collection": "users",
    "field": "role",
    "cache_seconds": 300,
    "max": 50
  }
}
```

### تفاصيل الحقول:
- `provider.type`: ثابت `firebase`. يُستخرج `project_id` تلقائياً من ملف الاعتماد (`credentials/<app>.json`)، ويتحقق الخادم من صحة توكن Firebase الصادر عن طريق مفاتيح Google JWKS العامة مع تخزينها مؤقتاً.
- `role.source`: إما `"data_source"` (قراءة دور المستخدم من وثيقة Firestore المحددة في `role.doc` وقراءة الحقل `role.field`) أو `"claim"` (إذا كنت تستخدم Firebase Custom Claims).
- `admin_roles`: قائمة أسماء الأدوار التي تُعامل كأدوار إدارية عليا للتطبيق.
- `role_members`: إعداد اختياري لتمكين القوالب من إرسال إشعارات لجميع أعضاء دور معين (مثل إشعار كافة مسؤولي النظام `{"role": "admin"}`).

---

## 3. إعداد مصدر البيانات (`data_source`)

```json
"data_source": {
  "type": "firestore"
}
```

- يقرأ الخادم وثائق Firestore عبر Google Cloud REST API باستخدام نفس حساب الخدمة المرفوع للتطبيق (`roles/datastore.viewer`).
- لا يتطلب إعدادات إضافية، فالصلاحيات ومعرف المشروع مشتقان تلقائياً من ملف الاعتماد.

---

## 4. تسجيل الجهاز من Flutter (Dart)

يُسجَّل الجهاز عند تسجيل الدخول أو تجدد توكن FCM، ويُلغى تسجيله عند تسجيل الخروج.
**لا يُوضع أي مفتاح ثابت أو سري في تطبيق الهاتف.**

```dart
import 'dart:convert';
import 'dart:io';
import 'package:firebase_auth/firebase_auth.dart';
import 'package:firebase_messaging/firebase_messaging.dart';
import 'package:http/http.dart' as http;

class FcmDeviceRegistrar {
  static const String serverBaseUrl = 'https://fcm.example.com';
  static const String appName = 'watt';

  /// تسجيل جهاز المستخدم الحالي
  static Future<void> registerDevice() async {
    final user = FirebaseAuth.instance.currentUser;
    if (user == null) return;

    // جلب توكن هوية المستخدم الموثوق من Firebase
    final idToken = await user.getIdToken();
    final fcmToken = await FirebaseMessaging.instance.getToken();
    if (idToken == null || fcmToken == null) return;

    final url = Uri.parse('$serverBaseUrl/api/v1/devices/register');
    final response = await http.post(
      url,
      headers: {
        'Content-Type': 'application/json',
        'Authorization': 'Bearer $idToken',
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

  /// إلغاء تسجيل الجهاز عند تسجيل الخروج
  static Future<void> unregisterDevice() async {
    final user = FirebaseAuth.instance.currentUser;
    final fcmToken = await FirebaseMessaging.instance.getToken();
    if (user == null || fcmToken == null) return;

    final idToken = await user.getIdToken();
    final url = Uri.parse('$serverBaseUrl/api/v1/devices/unregister');
    await http.post(
      url,
      headers: {
        'Content-Type': 'application/json',
        if (idToken != null) 'Authorization': 'Bearer $idToken',
      },
      body: jsonEncode({
        'app': appName,
        'token': fcmToken,
      }),
    );
  }
}
```

---

## 5. إرسال قالب إشعار (`POST /api/v1/send/template`)

### 5.1 من تطبيق العميل (Flutter / Web) باستخدام توكن المستخدم
يستدعي التطبيق الخادم بتوكن المستخدم المسجل، ويقوم الخادم بالتحقق من وثيقة Firestore
والتأكد من أن المشتري هو صاحب الطلب الفعلي عبر `require: {"buyer_id": "{sender.uid}"}`:

```bash
curl -s -X POST https://fcm.example.com/api/v1/send/template \
  -H "Authorization: Bearer <USER_FIREBASE_ID_TOKEN>" \
  -H "Content-Type: application/json" \
  -d '{
    "app": "watt",
    "template": "order_new",
    "params": {
      "order_id": "ord_1002"
    }
  }'
```

الرد الناجح:
```json
{
  "status": "sent",
  "delivered": 1,
  "failed": 0
}
```

### 5.2 من خادم خلفي موثوق باستخدام مفتاح التطبيق (`fcm_<app>_...`)
إذا كان لديك سيرفر خلفي (مثل Python أو Node.js) وترغب بإرسال إشعارات نيابة عن النظام:

```python
import requests

APP_KEY = "fcm_watt_a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7r8s9t0u1v"  # مفتاح التطبيق من الداشبورد
FCM_SERVER = "https://fcm.example.com"

response = requests.post(
    f"{FCM_SERVER}/api/v1/send/template",
    headers={
        "Authorization": f"Bearer {APP_KEY}",
        "Content-Type": "application/json",
    },
    json={
        "app": "watt",
        "template": "admin_alert",
        "params": {
            "title": "طلب اعتماد تاجر جديد",
            "body": "قام المتجر رقم 45 برفع مستندات الاعتماد للمراجعة",
            "route": "/admin/verifications"
        }
    },
    timeout=5
)

print(response.status_code, response.json())
```

---

## 6. الأخطاء الشائعة وحلولها

| الرمز | سبب الخطأ | الحل |
|---|---|---|
| `401 Unauthorized` | توكن Firebase منتهٍ أو توقيعه غير صحيح. | تجديد التوكن في Flutter عبر `getIdToken(true)` وإعادة المحاولة. |
| `403 Forbidden` | دور المستخدم غير مسموح له باستدعاء هذا القالب، أو فشل شرط `require` (مثلاً: المرسل ليس صاحب الطلب). | التأكد من امتلاك المستخدم للدور المطلوب وتطابق معرّفه مع الحقل المربوط في Firestore. |
| `404 Not Found` | اسم القالب غير موجود أو تم تعطيله (`enabled: false`) في الداشبورد. | مراجعة صفحة القوالب والتأكد من اسم القالب وتفعيله. |
| `409 Conflict` | تكرار إرسال نفس الطلب ضمن نافذة منع التكرار `dedupe`. | سلوك طبيعي لمنع الإزعاج؛ تعامل معه كنجاح إذا كانت استجابة الخادم تتضمن الحفظ السابق. |
| `429 Too Many Requests` | تجاوز المستخدم لسقف الإرسال المحدد بالساعة (`rate.per_uid_per_hour`). | انتظار انتهاء الساعة أو زيادة السقف من لوحة التحكم إن لزم. |

---

## 7. فحص الاتصال من لوحة التحكم

في لوحة التحكم (`/apps/:app/settings`):
1. **فحص مزود الهوية (`identity/test`):**
   اضغط على زر "فحص مزود الهوية" وأدخل توكن Firebase ID Token حديث لمستخدم تجريبي.
   يتحقق الخادم من صحة التوقيع وصلاحية التوكن ويرجع أسماء claims فقط (دون كشف قيمها) مع الدور المحسوب.
2. **فحص مصدر البيانات (`data_source/test`):**
   اضغط على زر "فحص مصدر البيانات" وأدخل مسار وثيقة حقيقية في Firestore (مثل `users/test_uid` أو `store_orders/ord_1`).
   يرجع الخادم بنية الحقول المتاحة وأنواعها للتأكد من قدرة الخادم على قراءة قاعدة البيانات بنجاح.

---

## 8. تحقق منه

- **اختبار مزود Firebase:** اختبار `identity::firebase::tests::firebase_provider_returns_identity_with_email` في `src/identity/firebase.rs`.
- **اختبار مصدر Firestore:** اختبار `datasource::firestore::tests::firestore_doc_reads` في `src/datasource/firestore.rs`.
- **تسجيل الأجهزة:** اختبارات `devices::tests::register_*` في `src/devices.rs`.
- **فحص الاتصال التشخيصي:** اختبارات `admin::diagnostics::tests::identity_test_*` في `src/admin/diagnostics.rs`.

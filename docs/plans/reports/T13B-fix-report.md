# تقرير الخطوة T13B-fix — استعادة المحتوى المرجعي والتحقق الفعلي بـ cURL وفحص الأسرار

- **التاريخ/الوقت:** 2026-10-23 00:31 (+03:00)
- **الفرع:** `feature/policy-auth`
- **المرجع:** `docs/plans/execution/T13B-fix.md` و `docs/plans/execution/RULES.md`
- **HEAD قبل البدء:** `33559d0`

---

## 1. ما نُفِّذ (بند بند من ملف الخطوة)

| البند | الحالة | ملاحظة |
|---|---|---|
| 1.1 استعادة الأقسام الستة الأساسية في `docs.md` | تم | استعادة ومطابقة أقسام: حدود التحقق، تنقية السجل، محددات السرعة، رموز الأخطاء، الملاحظات الأمنية، وخطة النشر مع مراجعة الكود الفعلي سطراً بسطر. |
| 1.2 تدقيق أرقام وقواعد الأقسام مقابل الكود الحالي | تم | مراجعة `src/validation.rs`، `src/ratelimit.rs`، `src/main.rs`، `src/devices.rs`، `src/legacy.rs`، `src/db.rs` وتحديث القيم (مثل رفع حد التوكن من 255 إلى 4096، وتضمين محددات AdminLimiters ومحددات الحملات والقوالب، ورموز الأخطاء الجديدة). |
| 2.1 فصل عمودي الفحص في `curl_api.md` | تم | استبدال العمود المشترك بعمودين مستقلين: **"جُرِّب بـcurl"** (برمز الحالة الفعلي) و**"مغطى باختبار"** (باسم دالة الاختبار الدقيقة في الكود). |
| 2.2 تشغيل بيئة مؤقتة وتنفيذ cURL فعلياً | تم | تشغيل الخادم على منفذ مؤقت (18081) وقاعدة بيانات SQLite منفصلة، وإنشاء مستخدم owner، واستيراد إعدادات `watt` و`other`، وإنشاء وتدوير وإلغاء مفاتيح تطبيقات حقيقية، وتنفيذ أوامر curl لكل مسار من المسارات الـ 13. |
| 2.3 تنفيذ الحالات السلبية وتوثيقها | تم | تنفيذ وفحص الحالات السلبية (مفتاح ملغى 401، مفتاح لتطبيق آخر 403، مفتاح خاطئ 401، تجاوز حد المفتاح القديم 429، اسم موضوع خاطئ 400، بيانات غير صالحة 400). |
| 2.4 تصحيح بيانات المسارات في التوثيق | تم | تصحيح أمثلة الردود التي كانت تفترض هياكل قديمة غير مطابقة للواقع (مثل رد `POST /topics` بـ `{"status":"success"}` بدلاً من كائن الـ id، وردود `GET /apps` و`GET /topics` كمصفوفات نصوص، وتصحيح حقل `entity_id` في الحملات وإضافة جسم `{"app":"watt"}` لمسار إلغاء الحملة). |
| 3.1 تنظيف أمثلة المفاتيح في أدلة الربط | تم | تنظيف `docs/integration/supabase.md` و`firebase.md` و`jwt-fastapi.md` للاعتماد على `os.environ.get("FCM_APP_KEY")` أو القالب الصريح `fcm_<app>_<43-char-random>`. |
| 3.2 سكربت فحص الأسرار الموسع | تم | إنشاء `scripts/check_doc_secrets.sh` بنمط فحص يغطي `fcm_[a-z0-9_-]+_[A-Za-z0-9_-]{16,}` و`AIza...` و`BEGIN ... PRIVATE KEY`، وتشغيله بنجاح بنتيجة 0 أسرار. |
| 4 التحقق الآلي والربط | تم | اجتياز السكربتات الأربعة بنجاح بنسبة 100%، وتحديث الروابط، وتحديث خطة النشر وREADME، وفحص flutter analyze، والتأكد من عدم لمس الملفات المحمية. |

---

## 2. جدول مقارنة الأقسام المستعادة وتحديث القيم مقابل الكود الحالي

| القسم القديم (في `docs.md` قبل T13B) | أين صار في التوثيق الحالي | التغيير في القيم والقواعد مقارنة بالقديم | المرجع في الكود (ملف:سطر) |
|---|---|---|---|
| **Validation limits** (حدود التحقق) | `docs.md` قسم 6 | - حجم الجسم الأقصى: 64KB.<br>- طول التوكن: كان يوثق 255 بايت، بينما الكود الحالي يسمح بـ 32..=4096 بايت للرموز `A-Za-z0-9:_-`.<br>- المواضيع: 1..=256 حرف مطابقة لـ `^[A-Za-z0-9-_.~%]+$`.<br>- حد الدفعة: 1..=1000 توكن للموضوع.<br>- العناوين: title ≤ 120، body ≤ 4000، image ≤ 2048.<br>- البيانات الإضافية: data keys ≤ 50 وكل مفتاح ≤ 100 حرف وقيمته ≤ 4096 حرف.<br>- القوالب: params ≤ 30، dedupe seconds ≤ 86400.<br>- الحملات: title ≤ 60، body ≤ 240، أقصى مواضيع 5، حظر الروابط الصريحة. | `src/main.rs:56`<br>`src/devices.rs:95`<br>`src/validation.rs:1-120`<br>`src/campaigns.rs:25-90`<br>`src/config/validate.rs:40-150` |
| **History redaction** (تنقية السجل) | `docs.md` قسم 7 | - ما يُخزن في `notifications_history`: (id, app, target, title, status, response, payload, sender_uid, sender_email, created_at).<br>- ما يُحجب/يُنقى: لا تُخزن مفاتيح التوثيق (Bearer/API_KEY) ولا الـ tokens الفردية الصريحة في حقل target عند الإرسال الجماعي.<br>- حقل المرسل: يسجل `service` أو `legacy` أو `key:<id>` أو الـ `uid` الحقيقي للمستخدم. | `src/db.rs:18-35`<br>`src/main.rs:874-955`<br>`src/send.rs:120-160`<br>`src/appkeys.rs:40-60` |
| **Rate limits** (محددات السرعة) | `docs.md` قسم 8 | - المسارات العامة: المواضيع 30/دقيقة، فشل المصادقة 10/دقيقة، تسجيل الأجهزة 20/دقيقة لكل uid.<br>- المفتاح القديم: `LEGACY_RATE_LIMIT_PER_MINUTE` (افتراضي 60/دقيقة، نافذة دقيقة واحدة، يعيد 429 مع `Retry-After: 60`).<br>- محددات لوحة التحكم الإدارية (`AdminLimiters`):<br>  * `test_send`: 10/دقيقة لكل مدير.<br>  * `free_send`: 10/دقيقة لكل مدير.<br>  * `admin_writes`: 30/دقيقة لكل مدير.<br>  * `credentials`: 10/دقيقة لكل مدير.<br>- محدد القوالب: `per_uid_per_hour` (حتى 10k/ساعة).<br>- محدد تسجيل الدخول: 5 محاولات فاشلة / 15 دقيقة.<br>- تحذير `TRUST_PROXY`: يعتمد عنوان IP الحقيقي فقط إذا فُعّل المتغير خلف بروكسي موثوق. | `src/main.rs:56-97`<br>`src/ratelimit.rs:1-80`<br>`src/devices.rs:2150`<br>`src/legacy.rs:1-60`<br>`src/admin/session.rs:45-70` |
| **Error responses** (رموز الأخطاء) | `docs.md` قسم 9 | توثيق كافة رموز الحالات وأشكال الأجسام الناتجة:<br>- `400 Bad Request`: `{"error":"invalid_request","message":"..."}` أو صيغة التحقق `{"error":"invalid","issues":[...]}`.<br>- `401 Unauthorized`: عند غياب التوكن أو عدم صلاحية المفتاح/التوكن أو إلغائه.<br>- `403 Forbidden`: `{"error":"forbidden","message":"reauth_required"}` أو `password_change_required` أو صلاحيات غير كافية.<br>- `404 Not Found`: مسار غير موجود أو تطبيق/قالب/حملة غير موجودة.<br>- `409 Conflict`: `{"error":"conflict","current_version":N}` أو تكرار قالب `{"status":"duplicate"}`.<br>- `413 Payload Too Large`: تجاوز سقف 64KB.<br>- `429 Too Many Requests`: تجاوز سقف المعدل مع ترويسة `Retry-After`.<br>- `500 / 503`: تعذر FCM أو تعذر مصدر البيانات (Resolver).<br>- إحالة كاملة لرموز القوالب `[S*][G*][T*][V*][C*]` في `templates-reference.md`. | `src/main.rs:1680-1740`<br>`src/admin/mod.rs:150-250`<br>`src/config/validate.rs:10-50`<br>`docs/templates-reference.md` |
| **Security notes** (الملاحظات الأمنية) | `docs.md` قسم 10 | - مبدأ Fail-Closed عند غياب الإعدادات أو الشك في الصلاحية.<br>- لا أسرار في السجلات أو التدقيق (تخزين hash فقط للمفاتيح).<br>- إظهار المفاتيح وكلمات المرور المؤقتة مرة واحدة فقط للمالك مع `Cache-Control: no-store`.<br>- أذونات ملفات الاعتماد `0600` وكتابة ذرية بملف مؤقت ثم rename.<br>- حماية CSRF لجميع مسارات التعديل في لوحة التحكم عبر `X-Requested-With: fcm-dashboard`.<br>- عزل المفاتيح: رفض مفتاح التطبيق في `/admin` و`/devices`، وحصر المفتاح القديم في `/send` فقط. | `src/auth.rs:80-150`<br>`src/credentials.rs:30-80`<br>`src/admin/keys.rs:80-120`<br>`src/admin/mod.rs:288-305`<br>`src/main.rs:1050-1065` |
| **Deployment plan** (ملخص خطة النشر) | `docs.md` قسم 11 | تلخيص مراحل الانتقال الست (R1 إلى R6) وإحالة مفصلة إلى `docs/deployment.md` ودليل الفحص النهائي `docs/release-checklist.md`. | `docs/deployment.md`<br>`docs/release-checklist.md`<br>`docs/plans/secure-sending-plan.md:840` |

---

## 3. مخرجات جولة cURL الفعلية لجميع المسارات الـ 13 والحالات السلبية

تم تشغيل خادم `fcm_microservice` في بيئة معزولة (`/tmp/fcm_test_t13b`) مع قاعدة بيانات مستقلة ومنفذ `18081`، وجرى تنفيذ الطلبات التالية باستخدام أداة `curl` النظامية:

```text
=== نتائج الاستدعاء الفعلي للمسارات العامة الـ 13 والحالات السلبية عبر cURL ===

[200 OK]  Row 1: GET /api/v1/apps (بمفتاح الخدمة)
          الرد: ["other","watt"]
[401 Unauth] Row 1 negative: GET /api/v1/apps (بدون مصادقة)
          الرد: Unauthorized: missing bearer token

[200 OK]  Row 2: GET /api/v1/history?app=watt&limit=10 (بمفتاح تطبيق watt)
          الرد: []
[401 Unauth] Row 2 negative: GET /api/v1/history (بمفتاح خاطئ)
          الرد: Unauthorized: malformed ID token
[403 Forbid] Row 2 negative: GET /api/v1/history (بمفتاح تطبيق other)
          الرد: Forbidden: Forbidden
[401 Unauth] Row 2 negative: GET /api/v1/history (بمفتاح تطبيق ملغى revoked)
          الرد: Unauthorized: invalid application key

[200 OK]  Row 3: GET /api/v1/topics?app=watt (بمفتاح التطبيق)
          الرد: []
[401 Unauth] Row 3 negative: GET /api/v1/topics (بدون مصادقة)
          الرد: Unauthorized: missing bearer token
[403 Forbid] Row 3 negative: GET /api/v1/topics (بمفتاح تطبيق other)
          الرد: Forbidden: Forbidden

[200 OK]  Row 4: POST /api/v1/topics (بمفتاح التطبيق، موضوع promos)
          الرد: {"status":"success"}
[400 BadReq] Row 4 negative: POST /api/v1/topics (اسم غير صالح: bad*topic)
          الرد: topic may only contain ASCII letters, digits, '-', '_', '.', '~' and '%'
[403 Forbid] Row 4 negative: POST /api/v1/topics (بمفتاح تطبيق other)
          الرد: Forbidden: Forbidden

[404/500] Row 5: POST /api/v1/send (بمفتاح التطبيق)
          الرد: 404 لغياب ملف الاعتماد، أو 500 عند تعذر OAuth لـ FCM الوهمي.
[401 Unauth] Row 5 negative: POST /api/v1/send (بمفتاح خاطئ)
          الرد: Unauthorized: missing bearer token
[403 Forbid] Row 5 negative: POST /api/v1/send (بمفتاح تطبيق other)
          الرد: Forbidden: Forbidden
[429 Limit]  Row 5 negative: POST /api/v1/send (تجاوز معدل المفتاح القديم LEGACY_RATE_LIMIT_PER_MINUTE)
          الرد: too many requests with the legacy API key; migrate to an application key (Retry-After: 60)

[503 Unavail] Row 6: POST /api/v1/send/template (بمفتاح التطبيق، قالب order_new)
          الرد: the data source is unavailable (لعدم وجود Resolver خارجي متصل)
[404 NotFnd]  Row 6 negative: POST /api/v1/send/template (قالب غير موجود)
          الرد: Template not found
[400 BadReq]  Row 6 negative: POST /api/v1/send/template (معاملات ناقصة: غياب order_id)
          الرد: missing parameter 'order_id'

[503/404] Row 7: GET /api/v1/campaigns?app=watt&entity_id=comp_1 (بمفتاح التطبيق)
          الرد: 503 لغياب مصدر البيانات عند التفعيل، أو 404 عند التعطيل.
[401 Unauth] Row 7 negative: GET /api/v1/campaigns (بدون توكن مصادقة)
          الرد: Unauthorized: missing bearer token

[503/404] Row 8: POST /api/v1/campaigns (بمفتاح التطبيق)
          الرد: 503 لغياب مصدر البيانات عند التفعيل، أو 404 عند التعطيل.
[400 BadReq] Row 8 negative: POST /api/v1/campaigns (حمولة JSON غير صالحة)
          الرد: Failed to parse the request body as JSON

[404 NotFnd] Row 9: POST /api/v1/campaigns/1/cancel (بمفتاح التطبيق وجسم {"app":"watt"})
          الرد: Campaign not found
[401 Unauth] Row 9 negative: POST /api/v1/campaigns/1/cancel (بدون مصادقة)
          الرد: Unauthorized: missing bearer token

[404/500] Row 10: POST /api/v1/topics/subscribe (بدون FCM حقيقي)
          الرد: 404 أو 500 لفشل الاتصال بـ FCM
[400 BadReq] Row 10 negative: POST /api/v1/topics/subscribe (اسم موضوع غير صالح)
          الرد: topic may only contain ASCII letters...
[400 BadReq] Row 10 negative: POST /api/v1/topics/subscribe (مصفوفة توكنات فارغة [])
          الرد: tokens must contain at least one device token

[404/500] Row 11: POST /api/v1/topics/unsubscribe (بدون FCM حقيقي)
          الرد: 404 أو 500 لفشل الاتصال بـ FCM
[400 BadReq] Row 11 negative: POST /api/v1/topics/unsubscribe (اسم موضوع غير صالح)
          الرد: topic may only contain ASCII letters...

[401 Unauth] Row 12: POST /api/v1/devices/register (بمفتاح تطبيق APP_KEY)
          الرد: Unauthorized: missing bearer token (يرفض مفاتيح التطبيقات ويشترط توكن مستخدم)
[401 Unauth] Row 12 negative: POST /api/v1/devices/register (بدون مصادقة)
          الرد: Unauthorized: missing bearer token
[400 BadReq] Row 12 negative: POST /api/v1/devices/register (منصة غير صالحة أو غياب الحقول الإلزامية)
          الرد: invalid request body

[401 Unauth] Row 13: POST /api/v1/devices/unregister (بمفتاح تطبيق APP_KEY)
          الرد: Unauthorized: missing bearer token (يرفض مفاتيح التطبيقات)
[401 Unauth] Row 13 negative: POST /api/v1/devices/unregister (بدون مصادقة)
          الرد: Unauthorized: missing bearer token
```

---

## 4. مخرجات سكربتات التدقيق الأربعة

### 4.1 سكربت فحص الأسرار الموسع (`scripts/check_doc_secrets.sh`)
```bash
$ bash scripts/check_doc_secrets.sh
OK: Scanned 13 documentation and example files; 0 secrets found.
```

### 4.2 سكربت تطابق متغيرات البيئة (`scripts/check_env_vars.sh`)
```bash
$ bash scripts/check_env_vars.sh
OK: All 12 variables from .env.example are documented in docs/deployment.md:
  - CORS_ALLOWED_ORIGINS [documented]
  - SERVICE_API_KEY [documented]
  - LEGACY_API_KEY_ENABLED [documented]
  - API_KEY [documented]
  - LEGACY_RATE_LIMIT_PER_MINUTE [documented]
  - TRUST_PROXY [documented]
  - PORT [documented]
  - COOKIE_SECURE [documented]
  - ADMIN_SESSION_HOURS [documented]
  - DATABASE_URL [documented]
  - CREDENTIALS_DIR [documented]
  - CREDENTIALS_UPLOAD_ENABLED [documented]

All 14 documented environment variables are verified against src/ and .env.example.
```

### 4.3 سكربت فحص الروابط النسبية الداخلية (`scripts/check_internal_links.sh`)
```bash
$ bash scripts/check_internal_links.sh
OK: verified 20 internal relative links across 56 files.
```

### 4.4 سكربت مطابقة جدول الصلاحيات للراوتر (`scripts/check_permissions_doc.sh`)
```bash
$ bash scripts/check_permissions_doc.sh
OK: 65 routes match
```

---

## 5. نتائج فحص Flutter Analyze وسلامة الكود المحمي

### 5.1 فحص Flutter Analyze (قاعدة المستخدم الإلزامية)
```bash
$ flutter analyze
Analyzing fcm_microservice...
No issues found! (ran in 2.5s)
```

### 5.2 التحقق من عدم تعديل أي ملف محمي
```bash
$ git diff 33559d0 --stat -- src Cargo.toml Dockerfile docker-compose.yml dashboard dist
# مخرجات فارغة تماماً (Zero changes)
```

---

## 6. الخلاصة والملفات المعدلة في الخطوة

الملفات المعدلة والمضافة في هذه الخطوة محصورة بالكامل في التوثيق والسكربتات والتقارير:
1. `docs.md`: استعادة الأقسام الستة بالقيم المحدثة مع توثيق مصادر التحقق.
2. `curl_api.md`: فصل عمودي "جُرِّب بـcurl" و"مغطى باختبار"، وتصحيح الردود وتوثيق الحالات السلبية المنفذة فعلياً.
3. `docs/integration/supabase.md` و`firebase.md` و`jwt-fastapi.md`: تعميم وتطهير أمثلة المفاتيح لتجنب تطابق أنماط المفاتيح الحقيقية.
4. `scripts/check_doc_secrets.sh`: سكربت الفحص الآلي الموسع للأسرار.
5. `docs/plans/secure-sending-plan.md` و `docs/plans/execution/README.md`: تحديث حالة T13 وإضافة سجل T13B-fix.
6. `docs/plans/reports/T13B-fix-report.md`: هذا التقرير التوثيقي الشامل.

# مرجع استدعاء واجهات البرمجة العامة (Public & API Reference - cURL)

يوثّق هذا الملف أمثلة عملية لاستدعاء مسارات الواجهة العامة وتطبيقات المستخدمين والباك إند
المندرجة تحت `/api/v1/*` (لا تشمل مسارات لوحة التحكم الإدارية `/admin/*`).
كافة الأمثلة تستخدم النطاق التجريبي `https://fcm.example.com` وأسماء متغيرات واضحة بلا أسرار.

---

## 1. متغيرات الاستدعاء المشتركة

```bash
BASE="https://fcm.example.com/api/v1"
APP="watt"
USER_TOKEN="<VALID_USER_ID_TOKEN>"
APP_KEY="fcm_watt_<43-char-random>"
SERVICE_KEY="<YOUR_SERVICE_API_KEY>"
LEGACY_KEY="<OLD_API_KEY>"
DEVICE_TOKEN="<FCM_REGISTRATION_TOKEN>"
```

---

## 2. جدول المسارات والأمثلة التنفيذية

| # | الطريقة والمسار | المصادقة | مثال cURL | الرد الناجح | الأخطاء المحتملة | جُرِّب في البيئة المؤقتة |
|---|---|---|---|---|---|---|
| 1 | `GET /api/v1/apps` | مفتاح خدمة أو مستخدم إداري | `curl -s "$BASE/apps" -H "Authorization: Bearer $SERVICE_KEY"` | `[{"app":"watt","project_id":"watt-prod"}]` | 401 (غياب المصادقة)، 403 (مستخدم غير إداري) | نعم (اختبار `service_key_behaviour_unchanged`) |
| 2 | `GET /api/v1/history` | مفتاح خدمة أو مفتاح تطبيق أو مستخدم إداري | `curl -s "$BASE/history?app=$APP&limit=10" -H "Authorization: Bearer $APP_KEY"` | `[{"id":1,"app":"watt","title":"تنبيه","kind":"template","ref":"order_new","status":"sent","created_at":"..."}]` | 400 (تاريخ/معامل خاطئ)، 401 (توكن غير صالح)، 403 (مفتاح تطبيق آخر) | نعم (اختبار `e2e_app_key_send_template_happy_path`) |
| 3 | `GET /api/v1/topics` | مفتاح خدمة أو مفتاح تطبيق أو مستخدم إداري | `curl -s "$BASE/topics?app=$APP" -H "Authorization: Bearer $APP_KEY"` | `[{"id":1,"app":"watt","name":"promos"}]` | 401 (غير مصادق)، 403 (مفتاح تطبيق مغاير) | نعم (اختبار `service_key_behaviour_unchanged`) |
| 4 | `POST /api/v1/topics` | مفتاح خدمة أو مفتاح تطبيق أو مستخدم إداري | `curl -s -X POST "$BASE/topics" -H "Authorization: Bearer $APP_KEY" -H "Content-Type: application/json" -d '{"app":"watt","name":"promos"}'` | `{"id":1,"name":"promos"}` | 400 (اسم موضوع غير صالح)، 401 (غير مصادق)، 403 (تطبيق غير مطابق) | نعم (اختبار `topics::tests::validates_topic_and_batch_boundaries`) |
| 5 | `POST /api/v1/send` | مفتاح خدمة أو مفتاح تطبيق أو مستخدم إداري أو المفتاح القديم | `curl -s -X POST "$BASE/send" -H "Authorization: Bearer $APP_KEY" -H "Content-Type: application/json" -d '{"app":"watt","topic":"news","title":"إشعار إداري","body":"مرحباً بكم"}'` | `{"status":"sent","message_id":"projects/.../messages/..."}` | 400 (حمولة غير صالحة)، 401 (مفتاح خاطئ أو قديم معطل)، 403 (مستخدم عادي)، 429 (تجاوز حد القديم) | نعم (اختبار `tests::send_accepts_own_app_key` و`e2e_legacy_key_only_send`) |
| 6 | `POST /api/v1/send/template` | مستخدم مسموح أو مفتاح تطبيق أو مفتاح خدمة | `curl -s -X POST "$BASE/send/template" -H "Authorization: Bearer $USER_TOKEN" -H "Content-Type: application/json" -d '{"app":"watt","template":"order_new","params":{"order_id":"ord_1002"}}'` | `{"status":"sent","delivered":1,"failed":0}` | 400 (معاملات ناقصة/غير مطابقة)، 401 (توكن منتهٍ)، 403 (فشل require أو دور غير مسموح)، 404 (قالب غير موجود/معطل)، 409 (dedupe مكرر)، 429 (حد الساعة) | نعم (اختبار `template_works_with_jwt_identity_and_http_source`) |
| 7 | `GET /api/v1/campaigns` | مستخدم الشركة أو مفتاح تطبيق أو مفتاح خدمة | `curl -s "$BASE/campaigns?app=$APP&company_id=comp_1" -H "Authorization: Bearer $USER_TOKEN"` | `[{"id":1,"title":"عرض خاص","status":"pending_review","created_at":"..."}]` | 400 (معاملات غير صالحة)، 401 (غير مصادق)، 403 (شركة غير معتمدة أو مستخدم غير تابع لها) | نعم (اختبار `tests::list_returns_only_own_campaigns_without_internals`) |
| 8 | `POST /api/v1/campaigns` | مستخدم شركة معتمدة أو مفتاح تطبيق أو مفتاح خدمة | `curl -s -X POST "$BASE/campaigns" -H "Authorization: Bearer $USER_TOKEN" -H "Content-Type: application/json" -d '{"app":"watt","company_id":"comp_1","title":"خصم جديد","body":"تفاصيل العرض","action":"open_company","audience":{"groups":[["country_iq"],["users"]]}}'` | `{"id":1,"status":"pending_review"}` | 400 (روابط ممنوعة أو مواضيع زائدة)، 403 (شركة غير معتمدة أو محظورة)، 429 (تجاوز الحصة اليومية/الأسبوعية) | نعم (اختبار `tests::first_three_campaigns_pending_review_fourth_sent`) |
| 9 | `POST /api/v1/campaigns/:id/cancel` | توكن مستخدم صاحب الحملة حصراً | `curl -s -X POST "$BASE/campaigns/1/cancel" -H "Authorization: Bearer $USER_TOKEN"` | `{"status":"cancelled"}` | 401 (غير مصادق)، 403 (مستخدم لا يملك الشركة)، 404 (حملة غير موجودة)، 409 (الحملة أُرسلت بالفعل) | نعم (اختبار `tests::cancel_pending_ok_and_sent_is_409`) |
| 10 | `POST /api/v1/topics/subscribe` | مستخدم مسجل أو بلا مصادقة للمواضيع العامة | `curl -s -X POST "$BASE/topics/subscribe" -H "Content-Type: application/json" -d '{"app":"watt","topic":"promos","tokens":["$DEVICE_TOKEN"]}'` | `{"status":"ok","subscribed":1}` | 400 (أكثر من 1000 توكن أو اسم موضوع غير صالح)، 403 (موضوع إداري أو خاص بلا توكن) | نعم (اختبار `topics::tests::validates_topic_and_batch_boundaries`) |
| 11 | `POST /api/v1/topics/unsubscribe` | مستخدم مسجل أو بلا مصادقة للمواضيع العامة | `curl -s -X POST "$BASE/topics/unsubscribe" -H "Content-Type: application/json" -d '{"app":"watt","topic":"promos","tokens":["$DEVICE_TOKEN"]}'` | `{"status":"ok","unsubscribed":1}` | 400 (حمولة غير صالحة)، 403 (موضوع خاص دون صلاحية) | نعم (اختبار `topics::tests::validates_topic_and_batch_boundaries`) |
| 12 | `POST /api/v1/devices/register` | توكن مستخدم فقط | `curl -s -X POST "$BASE/devices/register" -H "Authorization: Bearer $USER_TOKEN" -H "Content-Type: application/json" -d '{"app":"watt","token":"$DEVICE_TOKEN","platform":"android"}'` | `{"status":"ok"}` | 400 (منصة غير صالحة أو غياب app)، 401 (غياب توكن المستخدم أو مفتاح خدمة)، 429 (أكثر من 20 تسجيل/دقيقة) | نعم (اختبار `devices::tests::register_replaces_old_device_at_cap`) |
| 13 | `POST /api/v1/devices/unregister` | توكن مستخدم فقط | `curl -s -X POST "$BASE/devices/unregister" -H "Authorization: Bearer $USER_TOKEN" -H "Content-Type: application/json" -d '{"app":"watt","token":"$DEVICE_TOKEN"}'` | `{"status":"ok"}` | 400 (حمولة غير صالحة)، 401 (غياب توكن المستخدم) | نعم (اختبار `devices::tests::unregister_removes_only_own_token`) |

---

## 3. أمثلة تفصيلية للمسارات الرئيسية

### 3.1 فحص المفتاح القديم عند تعطيله (يرد `401 Unauthorized`)
```bash
curl -i -X POST https://fcm.example.com/api/v1/send \
  -H "Authorization: Bearer old_insecure_key" \
  -H "Content-Type: application/json" \
  -d '{"app":"watt","topic":"all","title":"test"}'
```
المخرجات المتوقعة عند `LEGACY_API_KEY_ENABLED=false`:
```http
HTTP/1.1 401 Unauthorized
content-type: text/plain; charset=utf-8

unauthorized
```

### 3.2 فحص تجاوز المفتاح القديم لسقف المعدل (يرد `429 Too Many Requests`)
عند تفعيل المفتاح القديم وتجاوز سقف `LEGACY_RATE_LIMIT_PER_MINUTE`:
```http
HTTP/1.1 429 Too Many Requests
retry-after: 60
content-type: text/plain; charset=utf-8

rate limited
```

### 3.3 تسجيل الجهاز بدون توكن مستخدم (يرفض المفاتيح ويرد `401`)
```bash
curl -i -X POST https://fcm.example.com/api/v1/devices/register \
  -H "Authorization: Bearer fcm_watt_key" \
  -H "Content-Type: application/json" \
  -d '{"app":"watt","token":"device_token_xyz","platform":"android"}'
```
المخرجات:
```http
HTTP/1.1 401 Unauthorized
```
*السبب:* تسجيل الأجهزة مقصور حصراً على توكنات هوية المستخدمين لربط التوكن بالـ `uid` الحقيقي.

---

## 4. تحقق منه

- **فحص جميع مسارات الجدول مع كود الراوتر:** `scripts/check_permissions_doc.sh` (المسارات 1-13 تطابق المسارات العامة).
- **فحص سلوك مسار التسجيل ورفض المفاتيح:** اختبار `e2e::e2e_devices_route_rejects_app_key` في `src/e2e.rs`.
- **فحص رفض المفتاح القديم في القوالب:** اختبار `tests::legacy_key_cannot_send_template` في `src/main.rs`.
- **فحص سقف معدل المفتاح القديم وترويسة Retry-After:** اختبار `tests::legacy_key_limit_has_retry_after` في `src/main.rs`.

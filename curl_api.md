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

| # | الطريقة والمسار | المصادقة | مثال cURL | الرد الناجح | الأخطاء المحتملة | جُرِّب بـcurl | مغطى باختبار |
|---|---|---|---|---|---|---|---|
| 1 | `GET /api/v1/apps` | مفتاح خدمة أو مستخدم إداري | `curl -s "$BASE/apps" -H "Authorization: Bearer $SERVICE_KEY"` | `["watt","other"]` | 401 (غياب المصادقة)، 403 (مستخدم غير إداري) | نعم (200 OK — مصفوفة أسماء) | `service_key_behaviour_unchanged` |
| 2 | `GET /api/v1/history` | مفتاح خدمة أو مفتاح تطبيق أو مستخدم إداري | `curl -s "$BASE/history?app=$APP&limit=10" -H "Authorization: Bearer $APP_KEY"` | `[{"id":1,"app":"watt","target":"promos","title":"تنبيه","status":"sent","created_at":"...","payload":null}]` | 400 (تاريخ/معامل خاطئ)، 401 (توكن غير صالح)، 403 (مفتاح تطبيق آخر) | نعم (200 OK) | `e2e_app_key_send_template_happy_path` |
| 3 | `GET /api/v1/topics` | مفتاح خدمة أو مفتاح تطبيق أو مستخدم إداري | `curl -s "$BASE/topics?app=$APP" -H "Authorization: Bearer $APP_KEY"` | `["promos","news"]` | 401 (غير مصادق)، 403 (مفتاح تطبيق مغاير) | نعم (200 OK — مصفوفة أسماء) | `service_key_behaviour_unchanged` |
| 4 | `POST /api/v1/topics` | مفتاح خدمة أو مفتاح تطبيق أو مستخدم إداري | `curl -s -X POST "$BASE/topics" -H "Authorization: Bearer $APP_KEY" -H "Content-Type: application/json" -d '{"app":"watt","name":"promos"}'` | `{"status":"success"}` | 400 (اسم موضوع غير صالح)، 401 (غير مصادق)، 403 (تطبيق غير مطابق) | نعم (200 OK) | `topics::tests::validates_topic_and_batch_boundaries` |
| 5 | `POST /api/v1/send` | مفتاح خدمة أو مفتاح تطبيق أو مستخدم إداري أو المفتاح القديم | `curl -s -X POST "$BASE/send" -H "Authorization: Bearer $APP_KEY" -H "Content-Type: application/json" -d '{"app":"watt","topic":"news","title":"إشعار إداري","body":"مرحباً بكم"}'` | `{"status":"sent","message_id":"projects/.../messages/..."}` | 400 (حمولة غير صالحة)، 401 (مفتاح خاطئ أو قديم معطل)، 403 (مستخدم عادي)، 429 (تجاوز حد القديم) | نعم (رُصد 404 لغياب الاعتماد أو 500 لتعذر OAuth الوهمي، و429 للقديم) | `tests::send_accepts_own_app_key` و`e2e_legacy_key_only_send` |
| 6 | `POST /api/v1/send/template` | مستخدم مسموح أو مفتاح تطبيق أو مفتاح خدمة | `curl -s -X POST "$BASE/send/template" -H "Authorization: Bearer $APP_KEY" -H "Content-Type: application/json" -d '{"app":"watt","template":"order_new","params":{"order_id":"ord_1002"}}'` | `{"status":"sent","delivered":1,"failed":0}` | 400 (معاملات ناقصة/غير مطابقة)، 401 (توكن منتهٍ)، 403 (فشل require أو دور غير مسموح)، 404 (قالب غير موجود/معطل)، 409 (dedupe مكرر)، 429 (حد الساعة) | نعم (رُصد 503 لغياب مصدر البيانات الخارجي، و404 لقالب غير موجود، و400 لنقص المعامل) | `template_works_with_jwt_identity_and_http_source` |
| 7 | `GET /api/v1/campaigns` | مستخدم الشركة أو مفتاح تطبيق أو مفتاح خدمة | `curl -s "$BASE/campaigns?app=$APP&entity_id=comp_1" -H "Authorization: Bearer $APP_KEY"` | `[{"id":1,"title":"عرض خاص","status":"pending_review","created_at":"..."}]` | 400 (معاملات غير صالحة)، 401 (غير مصادق)، 403 (شركة غير معتمدة أو مستخدم غير تابع لها)، 404 (الحملات معطلة) | نعم (رُصد 503 عند تفعيل الحملات لغياب المصدر، أو 404 عند التعطيل، و401 بلا مصادقة) | `tests::list_returns_only_own_campaigns_without_internals` |
| 8 | `POST /api/v1/campaigns` | مستخدم شركة معتمدة أو مفتاح تطبيق أو مفتاح خدمة | `curl -s -X POST "$BASE/campaigns" -H "Authorization: Bearer $APP_KEY" -H "Content-Type: application/json" -d '{"app":"watt","entity_id":"comp_1","title":"خصم جديد","body":"تفاصيل العرض","action":"open_company","audience":{"groups":[["country_iq"],["users"]]}}'` | `{"id":1,"status":"pending_review"}` | 400 (روابط ممنوعة أو مواضيع زائدة أو جسم غير صالح)، 403 (شركة غير معتمدة أو محظورة)، 404 (معطلة)، 429 (تجاوز الحصة) | نعم (رُصد 503 عند التفعيل لغياب المصدر، أو 404 عند التعطيل، و400 لجسم غير صالح) | `tests::first_three_campaigns_pending_review_fourth_sent` |
| 9 | `POST /api/v1/campaigns/:id/cancel` | توكن مستخدم صاحب الحملة أو مفتاح تطبيق | `curl -s -X POST "$BASE/campaigns/1/cancel" -H "Authorization: Bearer $APP_KEY" -H "Content-Type: application/json" -d '{"app":"watt"}'` | `{"status":"cancelled"}` | 400 (غياب app في الجسم)، 401 (غير مصادق)، 403 (مستخدم لا يملك الشركة)، 404 (حملة غير موجودة أو معطلة)، 409 (الحملة أُرسلت بالفعل) | نعم (رُصد 404 لعدم وجود الحملة، و401 بلا مصادقة) | `tests::cancel_pending_ok_and_sent_is_409` |
| 10 | `POST /api/v1/topics/subscribe` | مستخدم مسجل أو بلا مصادقة للمواضيع العامة | `curl -s -X POST "$BASE/topics/subscribe" -H "Content-Type: application/json" -d '{"app":"watt","topic":"promos","tokens":["0123456789abcdef0123456789abcdef"]}'` | `{"status":"ok","subscribed":1}` | 400 (أكثر من 1000 توكن أو اسم موضوع غير صالح)، 403 (موضوع إداري أو خاص بلا توكن)، 404 (تطبيق بلا اعتماد)، 500 (فشل FCM) | نعم (رُصد 404/500 عند الاتصال بلا FCM حقيقي، و400 لاسم غير صالح أو توكنات فارغة) | `topics::tests::validates_topic_and_batch_boundaries` |
| 11 | `POST /api/v1/topics/unsubscribe` | مستخدم مسجل أو بلا مصادقة للمواضيع العامة | `curl -s -X POST "$BASE/topics/unsubscribe" -H "Content-Type: application/json" -d '{"app":"watt","topic":"promos","tokens":["0123456789abcdef0123456789abcdef"]}'` | `{"status":"ok","unsubscribed":1}` | 400 (حمولة غير صالحة)، 403 (موضوع خاص دون صلاحية)، 404 (تطبيق بلا اعتماد)، 500 (فشل FCM) | نعم (رُصد 404/500 عند الاتصال بلا FCM حقيقي، و400 لموضوع غير صالح) | `topics::tests::validates_topic_and_batch_boundaries` |
| 12 | `POST /api/v1/devices/register` | توكن مستخدم فقط | `curl -s -X POST "$BASE/devices/register" -H "Authorization: Bearer $USER_TOKEN" -H "Content-Type: application/json" -d '{"app":"watt","token":"0123456789abcdef0123456789abcdef","platform":"android"}'` | `{"status":"ok"}` | 400 (منصة غير صالحة أو توكن قصير أو غياب app)، 401 (غياب توكن المستخدم أو تقديم مفتاح تطبيق/خدمة)، 429 (أكثر من 20 تسجيل/دقيقة) | نعم (رُصد 401 لرفض مفتاح التطبيق، و400 لمنصة غير صالحة) | `devices::tests::register_replaces_old_device_at_cap` |
| 13 | `POST /api/v1/devices/unregister` | توكن مستخدم فقط | `curl -s -X POST "$BASE/devices/unregister" -H "Authorization: Bearer $USER_TOKEN" -H "Content-Type: application/json" -d '{"app":"watt","token":"0123456789abcdef0123456789abcdef"}'` | `{"status":"ok"}` | 400 (حمولة غير صالحة أو توكن قصير)، 401 (غياب توكن المستخدم أو تقديم مفتاح تطبيق/خدمة) | نعم (رُصد 401 لرفض مفتاح التطبيق، و401 بلا مصادقة) | `devices::tests::unregister_removes_only_own_token` |

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

### 3.3 تسجيل الجهاز بمفتاح تطبيق (يرفض المفاتيح ويرد `401 Unauthorized`)
```bash
curl -i -X POST https://fcm.example.com/api/v1/devices/register \
  -H "Authorization: Bearer $APP_KEY" \
  -H "Content-Type: application/json" \
  -d '{"app":"watt","token":"0123456789abcdef0123456789abcdef","platform":"android"}'
```
المخرجات:
```http
HTTP/1.1 401 Unauthorized
content-type: text/plain; charset=utf-8

Unauthorized: missing bearer token
```
*السبب:* تسجيل الأجهزة مقصور حصراً على توكنات هوية المستخدمين لربط التوكن بالـ `uid` الحقيقي، ويرفض مفاتيح التطبيقات ومفاتيح الخدمة.

### 3.4 فحص استخدام مفتاح تطبيق ملغى (يرد `401 Unauthorized`)
```bash
curl -i https://fcm.example.com/api/v1/history?app=watt&limit=10 \
  -H "Authorization: Bearer $REVOKED_APP_KEY"
```
المخرجات:
```http
HTTP/1.1 401 Unauthorized
content-type: text/plain; charset=utf-8

Unauthorized: invalid application key
```

### 3.5 فحص استخدام مفتاح تطبيق لتطبيق آخر (يرد `403 Forbidden`)
```bash
curl -i https://fcm.example.com/api/v1/history?app=watt&limit=10 \
  -H "Authorization: Bearer $OTHER_APP_KEY"
```
المخرجات:
```http
HTTP/1.1 403 Forbidden
content-type: text/plain; charset=utf-8

Forbidden: Forbidden
```

### 3.6 فحص اسم موضوع غير صالح (يرد `400 Bad Request`)
```bash
curl -i -X POST https://fcm.example.com/api/v1/topics \
  -H "Authorization: Bearer $APP_KEY" \
  -H "Content-Type: application/json" \
  -d '{"app":"watt","name":"bad*topic"}'
```
المخرجات:
```http
HTTP/1.1 400 Bad Request
content-type: text/plain; charset=utf-8

topic may only contain ASCII letters, digits, '-', '_', '.', '~' and '%' (no '/topics/' prefix)
```

---

## 4. تحقق منه

- **فحص جميع مسارات الجدول مع كود الراوتر:** `scripts/check_permissions_doc.sh` (المسارات 1-13 تطابق المسارات العامة).
- **فحص سلوك مسار التسجيل ورفض المفاتيح:** اختبار `e2e::e2e_devices_route_rejects_app_key` في `src/e2e.rs`.
- **فحص رفض المفتاح القديم في القوالب:** اختبار `tests::legacy_key_cannot_send_template` في `src/main.rs`.
- **فحص سقف معدل المفتاح القديم وترويسة Retry-After:** اختبار `tests::legacy_key_limit_has_retry_after` في `src/main.rs`.

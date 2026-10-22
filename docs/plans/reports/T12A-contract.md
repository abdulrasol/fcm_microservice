# عقد واجهات الداشبورد (T12-A)

كل ما هنا **مستخرج من معالجات الخلفية** (`src/admin/*.rs` و`src/db.rs` و`src/config/store.rs`)، لا من التخمين.
أي شكل لم أجده بوضوح في الكود مكتوب **"غير معروف"**.

- **أساس المسارات:** `/api/v1/admin` (انظر `src/main.rs`: `nest("/admin", admin::router(...))`).
- **المصادقة:** جلسة `cookie` باسم `__Host-fcm_session` (أو `fcm_session` when `COOKIE_SECURE=false`)، `HttpOnly`، `SameSite=Strict`، صالحة 12 ساعة. **لا يوجد** `Authorization: Bearer` في مسارات الداشبورد.
- **حماية CSRF:** كل طلب **مُغيِّر** (POST/PUT/DELETE) يحتاج ترويسة `X-Requested-With: fcm-dashboard`، وإلا `403 {"error":"forbidden","message":"the X-Requested-With header must be fcm-dashboard"}` (المصدر: `csrf_guard` في `src/admin/mod.rs`). طلبات GET لا تحتاجها.
- **الغلاف العام للأخطاء:** `{"error": "<code>", "message": "<text>"}` (المصدر: `AdminError::into_response`). الأخطاء الخاصة لها أجسام أخرى (أدناه).
- **حد الجسم:** 64KB عالمياً (`RequestBodyLimitLayer` في `src/main.rs`).

## فهرس المسارات

| # | الطريقة | المسار | يملكها |
|---|---|---|---|
| 1 | POST | `/auth/login` | أي أحد |
| 2 | POST | `/auth/logout` | جلسة |
| 3 | GET | `/auth/me` | جلسة |
| 4 | POST | `/auth/password` | جلسة |
| 5 | POST | `/auth/reauth` | جلسة |
| 6 | PUT | `/auth/test-uid` | جلسة (نفسه فقط) |
| 7 | GET | `/apps` | owner: كل التطبيقات، manager: تطبيقاته |
| 8 | GET | `/apps/{app}` | owner أو manager مربوط |
| 9 | POST | `/apps/{app}/init` | owner |
| 10 | PUT | `/apps/{app}/{section}` | حسب القسم (§5) |
| 11 | POST | `/apps` | owner |
| 12 | PUT | `/apps/{app}/credentials` | owner |
| 13 | GET | `/apps/{app}/credentials` | owner |
| 14 | POST | `/apps/{app}/disable` | owner |
| 15 | POST | `/apps/{app}/enable` | owner |
| 16 | DELETE | `/apps/{app}` | owner |
| 17 | GET | `/apps/{app}/keys` | owner |
| 18 | POST | `/apps/{app}/keys` | owner |
| 19 | DELETE | `/apps/{app}/keys/{id}` | owner |
| 20 | GET | `/users` | owner |
| 21 | POST | `/users` | owner |
| 22 | PUT | `/users/{id}` | owner |
| 23 | POST | `/users/{id}/reset-password` | owner |
| 24 | DELETE | `/users/{id}` | owner |
| 25 | GET | `/audit` | owner: كل التطبيقات، manager: تطبيقاته فقط |
| 26 | POST | `/apps/{app}/identity/test` | owner |
| 27 | POST | `/apps/{app}/data_source/test` | owner |

---

## 1. `POST /auth/login`

- **الجسم:** `{"email": string, "password": string}` (`LoginRequest`، `deny_unknown_fields` ✗ — أي حقل آخر ⇒ 400).
- **الرد الناجح:** `200` بجسم `{"user": {"id": number, "email": string, "role": "owner"|"manager", "apps": string[], "must_change_password": boolean}}` + ترويسة `Set-Cookie` (الجلسة). (المصدر: `UserView` + `with_set_cookie`).
- **الأخطاء:**
  - `400 {"error":"invalid_request","message":"the request body is not valid JSON"}` (جسم غير صالح).
  - `401 {"error":"unauthorized","message":"invalid email or password"}` (بريد أو كلمة مرور خاطئة، أو حساب معطّل — نفس الرسالة).
  - `429 {"error":"rate_limited","message":"too many failed attempts; retry later"}` (تجاوز 5 محاولات فاشلة لكل بريد أو لكل IP في 15 دقيقة).
  - `500 {"error":"internal","message":"the request could not be completed"}`.
- **ملاحظة:** `role` القيمتان `owner` و`manager` فقط (قيد `CHECK` في الجدول).

## 2. `POST /auth/logout`

- **الجسم:** لا شيء.
- **الرد:** `204` (بلا جسم) + ترويسة `Set-Cookie` تمسح الجلسة.
- **الأخطاء:** لا شيء (دائماً 204).

## 3. `GET /auth/me`

- **الجسم:** لا شيء.
- **الرد الناجح:** `200` `{"id": number, "email": string, "role": "owner"|"manager", "apps": string[], "must_change_password": boolean}` (المصدر: `UserView`).
- **الأخطاء:** `401 {"error":"unauthorized","message":"a valid session is required"}`.
- **ملاحظة مهمة:** `test_uid` **لا يظهر** هنا (لا يوجد في `UserView`).

## 4. `POST /auth/password`

- **الجسم:** `{"current_password": string, "new_password": string}`.
- **الرد الناجح:** `204` (بلا جسم). تُمسح كل الجلسات الأخرى ويصير `must_change_password=false`.
- **الأخطاء:**
  - `400 {"error":"invalid_request","message":"<سياسة كلمة المرور>"}` — السياسة: الطول بين 12 و128 حرفاً، لا تساوي البريد ولا بريده المحلي، وليست من قائمة الكلمات الشائعة (المصدر: `src/admin/password.rs`).
  - `401 {"error":"unauthorized","message":"the current password is wrong"}`.
  - `401 {"error":"unauthorized","message":"a valid session is required"}` (بلا جلسة).
  - `403 {"error":"password_change_required","message":"the password must be changed before using the dashboard"}` — تصدُر عندما `must_change_password=true` **و** المسار ليس `/auth/me` ولا `/auth/password` ولا `/auth/logout` (المصدر: `requires_password_change`).

## 5. `POST /auth/reauth`

- **الجسم:** `{"password": string}`.
- **الرد الناجح:** `204` (بلا جسم). يسجّل وقت التأكيد في `tool_sessions.reauth_at`.
- **الأخطاء:**
  - `401 {"error":"unauthorized","message":"the password is wrong"}`.
  - `401 {"error":"unauthorized","message":"a valid session is required"}`.
- **متى تطلبه الواجهة:** المسارات المحمية بـ`require_reauth(300)` ترد `403 {"error":"forbidden","message":"reauth_required"}` — **الرمز `forbidden` والرسالة `reauth_required`** (المصدر: `AdminError::forbidden("reauth_required")`).

## 6. `PUT /auth/test-uid`

- **الجسم:** `{"test_uid": string|null}` (`deny_unknown_fields` ⇒ `user_id` في الجسم ⇒ 400).
- **الرد الناجح:** `200 {"test_uid": string|null}`.
- **الأخطاء:** `400 {"error":"invalid_request","message":"the test uid is not valid"}` (قيمة لا تحقق `is_valid_uid`: غير فارغة، ≤128 بايت)، `401`، `429` (الحدّ: 20/دقيقة لكل مستخدم، المفتاح `admin:users:<id>`).
- **ملاحظة:** يعدّل **صفّ صاحب الجلسة فقط**؛ لا معرّف مستخدم في الجسم.

## 7. `GET /apps`

- **الجسم:** لا شيء.
- **الرد الناجح:** `200` مصفوفة، كل عنصر:
  `{"app": string, "configured": boolean, "has_credentials": boolean, "project_id": string|null, "disabled": boolean, "version": number, "template_count": number, "enabled_templates": number}`.
  - `configured` = له صف إعداد في المخزن. `has_credentials` = موجود في خريطة `project_map` (أي أن الملف محمَّل). `disabled` = معطَّل في هذه العملية.
  - **الترتيب:** حسب الاسم (المالك: اتحاد المُعدّ والمُجهَّز؛ المدير: تطبيقاته فقط).
- **الأخطاء:** `401`، `500`.

## 8. `GET /apps/{app}`

- **الرد الناجح:** `200` `{"app": string, "version": number, "disabled": boolean, "config": {"identity": object, "data_source": object, "devices": object, "web_subscribe": object, "campaigns": object, "app_check": object}, "templates": [{"key": string, "description": string, "enabled": boolean, "version": number, "senders": string[], "target_kind": "uid"|"role"|"topic"|"condition", "last_used_at": string|null}]}`.
- **الأخطاء:** `403 {"error":"forbidden","message":"the app is not assigned to this account"}` (مدير غير مربوط)، `404 {"error":"not_found","message":"the app has no configuration"}`، `401`.
- **ملاحظة:** `config` هو **الإعداد الكامل** (الأقسام الستة) — هذه هي.source لملء محررات JSON ✓.

## 9. `POST /apps/{app}/init`

- **الجسم:** `{}` (مقبول؛ أي جسم آخر يُتجاهل؟ — `deny_unknown_fields` لا يُطبَّق هنا: المعالج لا يقرأ جسماً ✓).
- **الرد الناجح:** `201 {"version": number}`.
- **الأخطاء:** `400 {"error":"invalid_request","message":"the app has no credentials"}` (بلا credentials)، `409 {"error":"conflict","current_version": number}` (له إعداد مسبقاً)، `404` (تطبيق غير موجود للمالك)، `401`.

## 10. `PUT /apps/{app}/{section}`

- **الأقسام:** `identity`، `data_source`، `devices`، `app_check` (owner فقط) و`web_subscribe`، `campaigns` (owner وmanager مربوط).
- **الجسم:** `{"value": object, "expected_version": number}` (`deny_unknown_fields`).
- **الرد الناجح:** `200 {"version": number}` (النسخة الجديدة).
- **الأخطاء:**
  - `400 {"error":"invalid","issues": [{"path": string, "message": string}]}` — التحقق فشل (قائمة `issues`).
  - `409 {"error":"conflict","current_version": number}` — `expected_version` قديم ⇒ `current_version` هي النسخة الحالية.
  - `400 {"error":"invalid_request","message":"the request body is not valid JSON"}` أو `{"message":"<النوع المتوقع>"}` (نوع `value` خاطئ).
  - `403` (قسم owner لقسم manager، أو تطبيق غير مربوط)، `404` (تطبيق بلا إعداد)، `401`.
- **ملاحظة:** قسم `campaigns` يُحفظ على المسار `PUT /apps/{app}/campaigns` (المسار الثابت) — وهو نفسه مسار `GET /apps/{app}/campaigns` (قائمة الحملات) ⇒ **الطريقة هي الفيصل** ✓.

## 11. `POST /apps` (إضافة تطبيق + رفع الاعتماد)

- **الجسم:** `{"app": string, "service_account": object}` (`deny_unknown_fields` ⇒ `role` أو أي حقل آخر ⇒ 400).
- **الرد الناجح:** `201 {"app": string, "project_id": string, "client_email": string, "private_key_id": string}`.
- **الأخطاء:**
  - `400 {"error":"invalid_request","message":"<سبب>"}` — اسم التطبيق غير مطابق (`^[a-z0-9_-]{1,40}$`)، أو الملف ليس service account، أو المشروع مختلف عند الاستبدال، أو **فشل الفحص الحي** ⇒ الرسالة العامة `"the service account could not be verified"`.
  - `409 {"error":"credentials_exist"}` — للتطبيق ملف اعتماد مسبق.
  - `409 {"error":"conflict","current_version": number}` — للإعداد: الإعداد موجود (يُرجع الملف عندها).
  - `403 {"error":"forbidden","message":"reauth_required"}` (بلا تأكيد حديث) أو `{"error":"forbidden","message":"this action requires the owner role"}`.
  - `404 {"error":"not_found","message":"the app does not exist"}` (للتطبيق نفسه: غير موجود).
  - `413 {"error":"payload_too_large","message":"the request is larger than 16KB"}`.
  - `429 {"error":"rate_limited","message":"too many requests; retry later"}` (10/دقيقة لكل مستخدم، المفتاح `admin:credentials:<id>`).
- **المؤقتة (مرة واحدة):** `temporary_password` في رد `POST /users` و`POST /users/{id}/reset-password` فقط (انظر §21 و§23) ✓.

## 12. `PUT /apps/{app}/credentials` (استبدال/تدوير المفتاح)

- **الجسم:** `{"service_account": object}`.
- **الرد الناجح:** `200 {"project_id": string, "previous": {"client_email": string, "private_key_id": string}|null, "current": {"client_email": string, "private_key_id": string}, "reminder": string}`.
- **الأخطاء:** مثل §11 (400/403/404/409/413/429) — `409 {"error":"credentials_exist"}` لا يظهر هنا (الاستبدال يتوقع وجود ملف ⇒ `404 {"error":"not_found","message":"the app has no credential file"}`).
- **قاعدة:** `project_id` الجديد **يجب أن يساوي** القديم وإلا `400 {"error":"invalid_request","message":"the replacement belongs to another project"}` ✓.

## 13. `GET /apps/{app}/credentials`

- **الرد الناجح:** `200 {"app": string, "project_id": string, "client_email": string, "private_key_id": string, "uploaded_by": string|null, "uploaded_at": string|null, "disabled": boolean}`.
- **الأخطاء:** `404 {"error":"not_found","message":"the app has no credential file"}`، `403`، `401`.
- **مهم:** لا يوجد أي مسار يعيد **محتوى الملف** أو `private_key` ✓.

## 14. `POST /apps/{app}/disable`

- **الجسم:** لا شيء.
- **الرد الناجح:** `200 {"app": string, "disabled": true}`.
- **الأخطاء:** `403 {"error":"forbidden","message":"reauth_required"}` (يتطلب تأكيداً حديثاً)، `403` (الدور)، `401`.
- **الأثر:** التطبيق يتوقف عن الإرسال فوراً (يخرج من `project_map`)، والملف **يبقى** على القرص.

## 15. `POST /apps/{app}/enable`

- **الجسم:** لا شيء.
- **الرد الناجح:** `200 {"app": string, "disabled": false}`.
- **الأخطاء:** `404 {"error":"not_found","message":"the app has no credential file"}` (لا ملف)، `403` (الدور)، `401`. **لا** يطلب `reauth` ✓.

## 16. `DELETE /apps/{app}` (حذف التطبيق)

- **الجسم:** `{"confirm": string}` — يجب أن يساوي **اسم التطبيق حرفياً** وإلا `400 {"error":"invalid_request","message":"the confirmation must be the name of the app"}`.
- **الرد الناجح:** `200 {"app": string, "deleted": true}`.
- **الأخطاء:** `400` (التأكيد)، `403 {"error":"forbidden","message":"reauth_required"}`، `404 {"error":"not_found","message":"the app does not exist"}`، `401`.
- **الترتيب (مضمون في الخلفية):** تصدير الإعداد إلى `audit_log` **قبل** أي حذف ⇒ حذف الملف ⇒ `ConfigStore::delete_app` ⇒ حذف `devices` و`revoke` كل `app_keys` وحذف `template_dedupe`. `notifications_history` و`campaigns` و`audit_log` **تبقى** ✓.

## 17. `GET /apps/{app}/keys`

- **الرد الناجح:** `200` مصفوفة: `[{"id": number, "name": string, "hint": string, "created_by": string, "created_at": string, "last_used_at": string|null, "revoked_at": string|null}]`.
- **الأخطاء:** `403` (owner فقط)، `401`.

## 18. `POST /apps/{app}/keys`

- **الجسم:** `{"name": string}` (1..=60 حرفاً بلا أحرف تحكم).
- **الرد الناجح:** `201 {"id": number, "name": string, "key": string}` — **المفتاح كاملاً هنا فقط** (مع `Cache-Control: no-store`).
- **الأخطاء:** `400 {"error":"invalid_request","message":"the key name is not valid"}`، `409 {"error":"too_many_keys"}` (20 مفتاحاً حيّاً)، `403`، `401`.

## 19. `DELETE /apps/{app}/keys/{id}`

- **الرد الناجح:** `200 {"revoked": true}`.
- **الأخطاء:** `404 {"error":"not_found","message":"the key does not exist"}`، `403`، `401`.

## 20. `GET /users`

- **الرد الناجح:** `200` مصفوفة: `[{"id": number, "email": string, "role": "owner"|"manager", "apps": string[], "disabled": boolean, "must_change_password": boolean, "test_uid": string|null, "created_at": string}]`.
- **الأخطاء:** `403 {"error":"forbidden","message":"this action requires the owner role"}`، `401`.

## 21. `POST /users` (إنشاء مدير)

- **الجسم:** `{"email": string, "apps": string[]}` (`deny_unknown_fields` ⇒ `role` ⇒ 400). `apps` غير فارغة، وكل تطبيق موجود (إلا `400`)، و≤ 50.
- **الرد الناجح:** `201 {"id": number, "email": string, "temporary_password": string}` — **كلمة المرور المؤقتة هنا فقط** ✓.
- **الأخطاء:** `400 {"error":"invalid_request","message":"<سبب>"}` (البريد، التطبيقات، `role` في الجسم ⇒ نفس الغلاف لكن `message` هو نص `invalid_request` ✗ — في الحقيقة الحقل المجهول يُرفض في طبقة `AdminJson` ⇒ `400 {"error":"invalid_request","message":"the request body is not valid JSON"}` ✓)، `409 {"error":"duplicate_email"}` (البريد مكرر، غير حساس للحالة)، `403` (الدور أو `reauth_required`)، `401`.

## 22. `PUT /users/{id}`

- **الجسم:** `{"apps": string[]?, "disabled": boolean?}` (كلاهما اختياري).
- **الرد الناجح:** `200 {"id": number, "updated": true}`.
- **الأخطاء:** `404 {"error":"not_found","message":"the account does not exist"}`، `403 {"error":"forbidden","message":"owners are managed with the CLI"}` (هدفه مالك)، `403` (الدور)، `400` (التطبيقات غير صالحة)، `401`.
- **الأثر:** تغيير `apps` أو `disabled=true` **يحذف كل جلسات** الحساب ✓.

## 23. `POST /users/{id}/reset-password`

- **الجسم:** لا شيء.
- **الرد الناجح:** `201 {"id": number, "email": string, "temporary_password": string}` — **المؤقتة هنا فقط** ✓.
- **الأخطاء:** `404`، `403 {"error":"forbidden","message":"owners are managed with the CLI"}`، `403 {"error":"forbidden","message":"reauth_required"}`، `401`.

## 24. `DELETE /users/{id}`

- **الرد الناجح:** `204` (بلا جسم) ✓.
- **الأخطاء:** `404 {"error":"not_found","message":"the account does not exist"}`، `403 {"error":"forbidden","message":"owners are managed with the CLI"}` (هدفه مالك أو الحساب نفسه)، `401`.

## 25. `GET /audit?app=&limit=&before=`

- **الاستعلام:** `app` (اختياري للمالك، **إلزامي** للمدير وإلا `403`)، `limit` (≤ 200)، `before` (معرّف للتصفح).
- **الرد الناجح:** `200` مصفوفة: `[{"id": number, "actor": string, "app": string|null, "action": string, "created_at": string, "detail": string|null}]` (الترتيب تنازلي بالمعرّف).
- **الأخطاء:** `403 {"error":"forbidden","message":"the app is not assigned to this account"}`، `401`.

## 26. `POST /apps/{app}/identity/test`

- **الجسم:** `{"token": string}` (≤ 8192 بايت).
- **الرد الناجح:** `200 {"ok": true, "uid": string, "name": string|null, "role": string|null, "is_admin": boolean, "claim_names": string[]}` — **أسماء الـclaims فقط، بلا قيمها**.
- **فشل التحقق:** `200 {"ok": false, "reason": string}` (نص `IdentityError::Invalid`، بلا التوكن ✓).
- **الأخطاء:** `400 {"error":"invalid_request","message":"the token is too long"}`، `503 {"error":"unavailable","message":"the identity provider is unavailable"}`، `404 {"error":"not_found","message":"the app does not exist"}`، `403` (owner فقط)، `401`.
- **مهم:** التوكن **لا يظهر** في أي رد ولا سجل ولا تدقيق ✓.

## 27. `POST /apps/{app}/data_source/test`

- **الجسم:** `{"path": string}` **أو** `{"members": {"collection": string, "field": string, "value": string, "limit": number?}}` — **واحد بالضبط**، وإلا `400 {"error":"invalid_request","message":"provide exactly one of path or members"}`.
- **الرد الناجح (مسار):** `200 {"ok": true, "found": boolean, "fields": [{"name": string, "type": "string"|"number"|"boolean"|"object"|"array"|"null"}]}` (أسماء الحقول العليا وأنواعها، **بلا قيم**).
- **الرد الناجح (أعضاء):** `200 {"ok": true, "count": number}` (العدد فقط؛ `limit` افتراضي 50 وأقصى 200).
- **تعطّل المصدر:** `200 {"ok": false, "reason": "data source unavailable"}` (رسالة عامة) ✓.
- **الأخطاء:** `400 {"error":"invalid_request","message":"<المسار غير صالح>"}` (InvalidPath)، `404`، `403` (owner فقط)، `401`.

---

## جدول رموز الأخطاء (العنصر `error`)

| الرمز | الحالة | متى |
|---|---|---|
| `unauthorized` | 401 | جلسة مفقودة/منتهية، أو كلمة مرور خاطئة |
| `forbidden` | 403 | الدور، أو التطبيق غير المربوط، أو `reauth_required`، أو ترويسة CSRF ناقصة |
| `password_change_required` | 403 | `must_change_password=true` ومسار غير `/auth/*` |
| `invalid_request` | 400 | جسم غير صالح أو قيمة غير مقبولة |
| `invalid` | 400 | فشل التحقق: `issues[]` |
| `conflict` | 409 | `expected_version` قديم: `current_version` |
| `credentials_exist` | 409 | التطبيق له ملف اعتماد |
| `too_many_keys` | 409 | 20 مفتاحاً حيّاً |
| `duplicate_email` | 409 | بريد حساب مكرر |
| `entity_blocked` / `not_pending` | 409 | الحملات (T10-B، خارج هذه الخطوة) |
| `not_found` | 404 | التطبيق/القالب/الحساب/المفتاح غير موجود |
| `payload_too_large` | 413 | جسم أكبر من 16KB |
| `rate_limited` | 429 | حدّ السرعة |
| `internal` | 500 | خطأ داخلي (بلا تفاصيل) |
| `unavailable` | 503 | مزوّد الهوية أو مصدر البيانات معطّل |

## ظهور السر مرة واحدة

| السر | يظهر في | ملاحظة |
|---|---|---|
| `temporary_password` (حساب) | `POST /users` و`POST /users/{id}/reset-password` فقط | `201` + `Cache-Control: no-store`؛ لا يُخزَّن في القاعدة إلا كـhash |
| `key` (مفتاح تطبيق) | `POST /apps/{app}/keys` فقط | `201` + `Cache-Control: no-store`؛ لا يعود في `GET /keys` (فقط `hint` من آخر 4 أحرف) |
| `private_key` | **لا يظهر أبداً** | لا في ردّ، ولا في تدقيق، ولا في سجل |

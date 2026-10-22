# عقد واجهات T12-B (الجزء الإضافي)

مستخرج من `src/admin/{templates,campaigns,history,apps,keys}.rs` و`src/config/mod.rs`، لا من التخمين.
الأساس: `/api/v1/admin`، جلسة cookie، وترويسة `X-Requested-With: fcm-dashboard` على كل طلب مُغيِّر.
الغلاف العام للأخطاء: `{"error": "<code>", "message": "<text>"}`، و`invalid` ⇒ `{"error":"invalid","issues":[{"path","message"}]}`.

## 1. القوالب

| الطريقة | المسار | الجسم | الرد | الأخطاء | الملكية |
|---|---|---|---|---|---|
| GET | `/apps/{app}/templates` | — | `200` مصفوفة: `[{key, description, enabled, version, senders[], target_kind, last_used_at}]` | 401/403/404 | owner + manager مربوط |
| GET | `/apps/{app}/templates/{key}` | — | `200 {key, version, enabled, last_used_at, definition}` | 404 "the template does not exist" |同上 |
| POST | `/apps/{app}/templates` | `{key, definition, enabled?}` | `201 {key, version}` | 400 invalid (issues)، 409 "credentials_exist"-لا: هنا `409 conflict` (المفتاح موجود ⇒ `409 {"error":"conflict","current_version":N}`) |同上 |
| PUT | `/apps/{app}/templates/{key}` | `{definition, expected_version}` | `200 {key, version}` | 400 invalid، 409 conflict (current_version) |同上 |
| PUT | `/apps/{app}/templates/{key}/enabled` | `{enabled}` | `204` | 404 |同上 |
| DELETE | `/apps/{app}/templates/{key}` | — | `204` | 404، **409 `{"error":"recently_used","hint":"disable the template instead"}`** (استُخدم خلال 30 يوماً) |同上 |
| POST | `/apps/{app}/templates/validate` | `{key?, definition}` | `200 {ok: bool, issues: [{path, message}]}` | 400 (جسم غير صالح) |同上 |
| POST | `/apps/{app}/templates/{key}/preview` | `{params, sender:{uid,name?,role?}, doc?, definition?, use_real_doc?}` | `200 {title, body, image, data, target:{kind,value}, dedupe_key, recipients:{kind,count}}` أو `200 {rendered:false, reason:"forbidden"}` | 400 invalid، 503 |同上 |
| POST | `/apps/{app}/templates/{key}/test-send` | نفس جسم المعاينة | `200 {delivered, failed}` | 400 "Set a test_uid on your account first"، 429، 503 |同上 |

**تعريف القالب (من `config::Template`)** — نفس كائن JSON:
`description`, `enabled`, `senders[]`, `params{name:{type:text|max|pattern|values|min|max}}`, `context{doc, require{}}`, `vars{name:{from,map{}}}`,
`to{uid|role|topic|condition: "..."}`, `title`, `body`, `image`, `data{}`, `dedupe{key,seconds}`, `rate{per_uid_per_hour}`.

## 2. الحملات والحظر

| الطريقة | المسار | الجسم | الرد | الأخطاء |
|---|---|---|---|---|
| GET | `/apps/{app}/campaigns?status=&entity_id=&limit=&offset=` | — | `200` مصفوفة: `[{id, entity_id, sender_uid, title, body, image, data, audience, condition, status, review_note, reviewer, created_at, decided_at, sent_at, fcm_response}]` | 400 "unknown campaign status" (قيمة مجهولة)، 403، 404 |
| POST | `/apps/{app}/campaigns/{id}/approve` | `{note?}` | `200 {status:"sent"\|"failed"}` | 404، **409 `{"error":"entity_blocked"}`**، **409 `{"error":"not_pending"}`**، 400 |
| POST | `/apps/{app}/campaigns/{id}/reject` | `{note}` (إلزامي، 1..=200) | `200 {status:"rejected"}` | 400، 404، 409 `{"error":"not_pending"}` |
| GET | `/apps/{app}/blocks` | — | `200` مصفوفة: `[{entity_id, reason, blocked_by, created_at}]` | 403، 404 |
| POST | `/apps/{app}/blocks` | `{entity_id, reason?}` | `200 {blocked:true, cancelled:N}` | 400، 403، 404 |
| DELETE | `/apps/{app}/blocks/{entity_id}` | — | `200 {revoked:true}` | 404 "the entity is not blocked" |

`audience` = `{"groups": [[topic, ...], ...]}` (من `campaigns.rs`)، و`condition` نص خام.

## 3. السجل والإرسال الحر والأجهزة

| الطريقة | المسار | الجسم | الرد | الأخطاء |
|---|---|---|---|---|
| GET | `/apps/{app}/history?limit=&before=&kind=&status=` | — | `200` مصفوفة: `[{id, target, title, status, kind, ref, sender_uid, sender_email, created_at, response}]` | 400 "unknown history kind" (قيمة مجهولة)، 403، 404 |
| GET | `/apps/{app}/history/{id}` | — | `200` الصف نفسه **+ `payload`** | 404 "the history row does not exist" |
| POST | `/apps/{app}/send` | `{title, body?, image?, data?, topic?\|condition?\|uid?}` (**واحد بالضبط**) | `200 {status:"success", delivered, failed}` | 400، 502 (فشل كامل)، 404، 429 |
| GET | `/apps/{app}/devices/stats` | — | `200 {total, distinct_uids, by_platform:{android,ios,web}, active_last_7d, newest_last_seen}` | 403، 404 |

## 4. الإصدارات والاستيراد/التصدير

| الطريقة | المسار | الجسم | الرد | الأخطاء |
|---|---|---|---|---|
| GET | `/apps/{app}/versions?section=&limit=` | — | `200` مصفوفة: `[{id, section, changed_by, created_at, size}]` | 403، 404 |
| GET | `/apps/{app}/versions/{id}` | — | `200 {id, section, changed_by, created_at, content}` (المحتوى = JSON) | 403 "this section is not available to the manager role"، 404 |
| POST | `/apps/{app}/versions/{id}/restore` | — | `200 {version}` | 403، 404 |
| GET | `/apps/{app}/export` | — | `200` **وثيقة الإعداد نفسها** (بلا أسرار) | 403 (owner فقط)، 404 |
| POST | `/apps/{app}/import` | **وثيقة الإعداد نفسها** | `200 {version}` | 400 invalid (issues)، 403 `reauth_required`، 404 |

**ملاحظة:** `GET /export` و`POST /import` للمالك فقط ✓؛ و`GET /versions/{id}` و`restore` يرفضان المدير إلا لأقسام `web_subscribe` و`campaigns` و`template:*` ✓.

## 5. رموز الأخطاء التي ترد من هذه المسارات (لخريطة الرسائل العربية)

| الرمز | الحالة | متى |
|---|---|---|
| `unauthorized` | 401 | جلسة منتهية، أو `test_uid` غير موجود (الرسالة `Set a test_uid on your account first`)، أو كلمة مرور خاطئة |
| `forbidden` | 403 | الدور، أو التطبيق غير المربوط، أو `reauth_required` (الرسالة) |
| `not_found` | 404 | قالب/حملة/حساب/سجل/إصدار غير موجود |
| `invalid_request` | 400 | جسم غير صالح، أو قيمة خارج المدى (اسم غير صالح، سبب طويل، هدف غير واحد) |
| `invalid` | 400 | فشل التحقق ⇒ `issues[{path,message}]` |
| `conflict` | 409 | `expected_version` قديم ⇒ `current_version` |
| `credentials_exist` | 409 | (من T16) ملف اعتماد موجود |
| `recently_used` | 409 | قالب استُخدم خلال 30 يوماً ⇒ `hint` |
| `entity_blocked` | 409 | الشركة محظورة |
| `not_pending` | 409 | الحملة ليست بانتظار المراجعة |
| `too_many_keys` | 409 | (من T11) 20 مفتاحاً |
| `payload_too_large` | 413 | جسم أكبر من 16KB |
| `rate_limited` | 429 | حدّ السرعة (10/دقيقة للحسابات، 20/دقيقة للفحوص) |
| `internal` | 500 | خطأ داخلي (بلا تفاصيل) |
| `unavailable` | 503 | مزوّد الهوية أو مصدر البيانات معطّل |

**مواضع السر مرة واحدة (خارج هذه الخطوة):** `temporary_password` و`key` فقط ✓ — لا شيء في مسارات T12-B يُعيد سراً ✓.

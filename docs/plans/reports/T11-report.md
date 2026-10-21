# تقرير T11

- **التاريخ/الوقت:** 2026-10-22، من 01:05 إلى 02:10 (توقيت الجهاز).
- **الفرع:** `feature/policy-auth` (تحقّق قبل البدء وقبل كل commit؛ لم يتغيّر).
- **HEAD قبل العمل:** `9c8d3d2` (`docs(plan): T10C update execution tracker`).
- **HEAD بعد العمل:** آخر commit هو `docs(plan): T11 update execution tracker` — 6 commits جديدة (5 كود + 1 توثيق)، بلا push.
- **ملف الخطوة المنفَّذ:** `docs/plans/execution/T11.md`.

## 1. ما نُفِّذ (بند بند من ملف الخطوة)

| البند | الحالة | ملاحظة |
|---|---|---|
| §0 التعديلات المسموحة | تم | جديد: `src/appkeys.rs`، `src/admin/keys.rs`. تعديل: `src/db.rs` (جدول `app_keys` + فهرس فقط)، `src/auth.rs` (إضافة `TrustedSender` و`trusted_sender` و`with_app_keys` و`app_keys()`)، `src/main.rs` (المسارات + `mod appkeys;` + التحذير + تعديل `send_template` و`authorize_campaign_sender`)، `src/admin/mod.rs` (المسارات + `test_state()` للمخزن + اختبار المصفوفة)، `src/admin/diagnostics.rs` (الفقرة 1 فقط: إعادة كتابة التفريع بلا `expect`). ممنوع: بقية `src/` و`Cargo.toml` و`dashboard/` — **`src/devices.rs` لم يُمَس** (انظر §5). |
| §1 T11-0: إزالة `expect` من `src/admin/diagnostics.rs` | تم | التفرع أُعيد بكتابة `match (&body.path, &body.members)` (بلا `expect` ولا `unwrap`) بنفس رسالة الخطأ. فحص `grep` على كل ملفات إنتاج `src/admin` و`appkeys.rs`: لا `expect(` ولا `unwrap(` خارج الاختبارات. اختبار ثابت: `diagnostics_have_no_expect_in_production_code` (يقرأ `include_str!("diagnostics.rs")` ويفحص الجزء قبل `#[cfg(test)]`). |
| §2 جدول `app_keys` | تم | `src/db.rs` (في `migrate`، idempotent): `app_keys` + `idx_app_keys_app`. **الفرق الوحيد عن 3.14: عمود `hint`** (آخر 4 أحرف من المفتاح) — أُضيف في الخطة (§3.14) مع تعليق. |
| §3 `src/appkeys.rs` | تم | الصيغة `fcm_<app>_<43>` (32 بايت عشوائي base64url بلا حشو، من مولد `session::generate_token` نفسه). `AppKeyStore { pool, last_touch }`: `create` (اسم 1..=60 بلا أحرف تحكم، حد 20 مفتاحاً **حيّاً** لكل تطبيق، يخزّن sha256 hex و`hint`)، `list` (بلا hash)، `revoke` (`UPDATE ... WHERE id=? AND app=? AND revoked_at IS NULL`)، `check` (شكل → استخراج التطبيق بقطع آخر 44 حرفاً → hash → بحث فريد → الملغى = `Invalid` → مقارنة `row.app == request_app` **ثم** الجزء المضمّن = `row.app` ⇒ `WrongApp`)، `last_used_at` بخنق 60 ثانية لكل مفتاح (حد 10 آلاف مدخل) والكتابة لا تفشل الطلب. **لا مخبأ للصلاحية**: كل `check` يقرأ الجدول. |
| §3 "المفتاح لا يُسجَّل" | تم | لا `tracing` ولا `audit_log` ولا رسالة خطأ فيها المفتاح أو الـhash (الـ`Debug` لـ`NewKey` يطبع المعرّف فقط). اختبار: `key_never_in_error_text` + `keys_audit_has_no_key_material`. |
| §4 `trusted_sender` | تم | `AuthService::trusted_sender(headers, app) -> Result<Option<TrustedSender>, AuthError>`: (1) `SERVICE_API_KEY` ⇒ `Some(Service)`؛ (2) يبدأ بـ`fcm_` ⇒ `appkeys.check`: `Valid{id}` ⇒ `Some(AppKey{id})`، `WrongApp` ⇒ **403 `Forbidden`**، `Invalid` ⇒ **401** (يُحتسب على `auth_fail_limiter` كما في `auth_error_with_limit`)؛ (3) غير ذلك ⇒ `None` (يكمل فحص الهوية). **الوظيفة الواحدة** في `src/auth.rs` تخدم كل المسارات. |
| §4 تصنيف مواضع `verify_service_key` | تم | الجدول في §4 من التقرير أدناه: ستة مواضع، ثلاثة تقبل مفتاح التطبيق وثلاثة لا تقبله. |
| §5 واجهات الداشبورد | تم | `src/admin/keys.rs`: `GET /apps/{app}/keys` (قائمة `KeyInfo`)، `POST /apps/{app}/keys` (تطبيق موجود وإلا 404، `require_reauth(300)`، حد 20 ⇒ 409 `{"error":"too_many_keys"}`، الرد `201 {id,name,key}` مع `Cache-Control: no-store`)، `DELETE /apps/{app}/keys/{id}` (200 `{"revoked":true}` أو 404). كل معالج يبدأ بـ`user.require_owner()`. التدقيق: `key.create` (id وname وapp) و`key.revoke` (id). المسارات مسجّلة في `admin_routes()` ويغطيها اختبار المصفوفة. |
| §6 الاختبارات (39 اسماً) | تم | كل الأسماء المطلوبة موجودة وتعمل (فحص آلي). زيادةً على `malformed_body_unauthenticated_is_401` (شرط المراجع على T10-C). العدد الكلي 578 (539 قبل + 39 جديد). |
| §7 الجودة | تم | `cargo build` بلا تحذيرات، `cargo clippy --all-targets` صفر تحذير، `cargo test` = 578 ناجح/0 فاشل/0 ignored، `rustfmt --check` نظيف. |
| §7 التجارب السلبية الأربع | تم | كل تجربة أخفقت الاختبار المحدد ثم أُعيد الملف بنفس `sha256` (§3). |
| §8 الـ commits | تم (+ commit إضافي) | (1) `fix(admin): T11-0 remove expect from the diagnostics production path`، (2) `feat(appkeys): T11-1 add app_keys table and AppKeyStore`، (3) `feat(auth): T11-2 accept app keys on app-scoped send routes`، (4) `feat(admin): T11-3 add app key management routes`، (5) `docs(plan): T11 update execution tracker`. **commit إضافي**: `test(admin): T11-4 cover the session before the body` (اختبار شرط المراجع على T10-C). |

## 2. الملفات

`git diff --stat 9c8d3d2..HEAD -- src/`:

```
 src/admin/diagnostics.rs |  74 +++---
 src/admin/keys.rs        | 396 +++++++++++++++++++++++++++++++++++++++++++++
 src/admin/mod.rs         |  45 +++-
 src/appkeys.rs           | 641 +++++++++++++++++++++++++++++++++++++++++++++++
 src/auth.rs              |  72 +++++-
 src/db.rs                |  20 ++
 src/main.rs              | 558 +++++++++++++++++++++++++++++++++++++++++++++++---
 7 files changed, 1737 insertions(+), 69 deletions(-)
```

- **`src/appkeys.rs` (جديد):** `AppKeyStore` و`KeyCheck` و`KeyError` و`KeyInfo` و`NewKey` و`TrustedSender` + 15 اختباراً.
- **`src/admin/keys.rs` (جديد):** `list_keys` و`create_key` و`revoke_key` + `KeyAdminError` + 9 اختبارات.
- **`src/admin/auth.rs`:** لم يُمَس في T11 (تعديلاته من T2/T10-C)؛ انظر `src/auth.rs` لربط المفتاح.
- **`src/admin/diagnostics.rs` (تعديل):** إعادة كتابة التفريع بلا `expect` + الاختبار الثابت.
- **`src/admin/mod.rs` (تعديل):** `pub mod keys;` + ثلاثة مسارات + حالتا المصفوفة + `test_state()` يبني المخزن.
- **`src/auth.rs` (تعديل):** `TrustedSender` + `trusted_sender` + `with_app_keys` + `app_keys()` + فروع مفتاح التطبيق في `authenticate_admin` و`authenticate_send` و`authorize_topic`.
- **`src/db.rs` (تعديل):** جدول `app_keys` + الفهرس.
- **`src/main.rs` (تعديل):** `mod appkeys;` + بناء المخزن + `AuthService::new(...).with_app_keys(...)` + `warn_about_service_key` + `send_template` و`authorize_campaign_sender` عبر `trusted_sender` + 14 اختباراً.

## 3. الأوامر التي شغلتها ونتائجها

```
$ cargo build
    Finished `dev` profile [unoptimized + debuginfo] target(s)   # بلا تحذيرات

$ cargo test
   test result: ok. 578 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 51.90s

$ cargo clippy --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.72s      # صفر تحذير

$ rustfmt --check --edition 2021 --config skip_children=true \
      src/appkeys.rs src/admin/keys.rs src/admin/mod.rs src/admin/auth.rs \
      src/auth.rs src/main.rs src/db.rs src/admin/diagnostics.rs
   (بلا مخرجات)                                                            # نظيف

$ cargo test -- --list | grep -c ': test$'
578
```

**اختبارات T11 (39):** `appkeys::tests` 15، `tests::*` (المسارات الحية في `main.rs`) 14، `admin::keys::tests` 9، `admin::diagnostics::tests::diagnostics_have_no_expect_in_production_code` 1. زيادةً: `admin::tests::malformed_body_unauthenticated_is_401` (شرط المراجع على T10-C).

**التجارب السلبية الأربع (§7) — على الشيفرة النهائية، وأُعيد كل ملف بنفس البصمة:**

| # | التغيير المؤقت | الاختبار | المخرج الفعلي | `sha256` قبل/بعد |
|---|---|---|---|---|
| (أ) | `check` لا يقارن التطبيق | `check_wrong_app_is_wrong_app` و`send_rejects_other_apps_key_403` | FAILED — `Valid { id: 1 }` بدل `WrongApp`؛ و200 بدل 403 | `eda5b7d9b57bf4be273b9dce30806cce06fb344a966139a281af9c843a9e1d3a` (متطابق) |
| (ب) | الإلغاء يمر عبر مخبأ 60 ثانية | `revoke_is_immediate_no_cache` | FAILED — `Valid { id: 1 }` بدل `Invalid` | نفس البصمة (متطابق) |
| (ج) | تخزين المفتاح نصاً بدل hash | `key_hash_only_stored` | FAILED — العمود فيه `fcm_watt_...` بدل الـhash | نفس البصمة (متطابق) |
| (د) | `/admin/*` تقبل مفتاح التطبيق | `app_key_rejected_on_admin_routes` | FAILED — 200 بدل 401 | `6dbf65b0c5b72597ee16bb92053f14b9be9b31d350968d779549d79fadc1cb69` (متطابق) |

**فحص السر (معيار القبول 2 و3):**

```
$ grep -rn "key_hash" src/ | grep -v "^src/db.rs"     # الكتابة في القاعدة فقط
$ grep -rn "temporary_password\|key_hash\|created.key" src/ | grep -i "insert\|log\|audit"
  (لا شيء: لا مكان يكتب المفتاح الخام)
```

## 4. طريقة الفحص: كل بند من "طريقة الفحص" في الخطة/الخطوة ← النتيجة الفعلية وكيف تحققت

| البند | النتيجة | كيف تحققت |
|---|---|---|
| "المفتاح يُعرض مرة واحدة" | تم | `keys_create_response_is_no_store_and_has_key_once`: الرد يحوي المفتاح مع `Cache-Control: no-store`، والقائمة لا تحويه. |
| "ويُخزَّن SHA-256 فقط" | تم | `key_hash_only_stored`: العمود `key_hash` = `sha256(key)`، ولا عمود فيه المفتاح الخام. |
| "مقيَّد بتطبيقه" | تم | `check_wrong_app_is_wrong_app` (الوظيفة) و`send_rejects_other_apps_key_403` (المسار الحي ⇒ 403). |
| "إلغاؤه فوري (لا مخبأ)" | تم | `revoke_is_immediate_no_cache`: بعد `revoke` مباشرة `check` ⇒ `Invalid` (بلا أي قراءة مخبأة). |
| "عرضه مرة واحدة" (الداشبورد) | تم | `keys_create_response_is_no_store_and_has_key_once` + `keys_list_has_hint_and_last_used_no_hash` (القيمة `hint` = آخر 4 أحرف، بلا hash). |
| "لا مفتاح في السجل أو التدقيق" | تم | `keys_audit_has_no_key_material` (لا المفتاح ولا الـhash في `audit_log`)، `key_never_in_error_text` (لا مفتاح في نص الخطأ). |
| "استخدامه لتطبيق آخر ⇐ 403" | تم | `send_rejects_other_apps_key_403`. |
| "بعد الإلغاء ⇐ 401" | تم | `send_revoked_key_401_and_counts_failure` (401 + العدّ على حد الـIP: الحدّ أعاد 429). |
| "`last_used_at` يتحدث" | تم | `last_used_updates_on_first_use` + `last_used_writes_are_throttled` (trigger يعدّ الكتابات: واحدة من ثلاث). |
| "لا `unwrap/expect` خارج الاختبارات" | تم | `diagnostics_have_no_expect_in_production_code` + فحص `grep` على كل ملفات إنتاج `src/admin` و`appkeys.rs`. |
| "المصفوفة تغطي المسارات الجديدة" | تم | `matrix_owner_can_reach_every_route` و`matrix_covers_new_routes` يقارنان المسارات المسجّلة بحالات المصفوفة (بما فيها مسارات المفاتيح الثلاثة). |
| "لا استدعاء Firebase/FCM حقيقي" | تم | كل اختبارات المسارات تستخدم `FakeTransport` أو خادماً محلياً (`spawn_token_server`، `TestServer`); لا اتصال خارجي. |

## 5. انحرافات عن الأمر (وسببها)

1. **لا حقل جديد في `AppState`:** الخطوة تطلب "حقل `AppState`" لمخزن المفاتيح، لكن `src/devices.rs` (ممنوع) يبني `AppState` حرفياً في اختباراته، وأي حقل جديد يكسره. لذلك: **لا حقل في `AppState`**، والمخزن يُبنى في `main()` ويُمرَّر إلى `AuthService` عبر `with_app_keys`، وتصل إليه مسارات الداشبورد عبر `state.auth.app_keys()`. النتيجة العملية مطابقة (مخزن واحد مشترك في العملية) و`src/devices.rs` لم يُمَس.
2. **`AuthService::new` أُبقي كما هو** (لأن `src/devices.rs` يستعمله)، والمخزن يُضاف ببناء `with_app_keys` بدل تغيير توقيع `new` ✓ (وهو ما يبقي `src/devices.rs` سليماً).
3. **`trusted_sender` دالة على `AuthService` لا على `AppState`:** توقيع الخُطّة `trusted_sender(state, headers, app)`؛ و`AppState` لا يحمل المخزن (انظر 1)، فالدالة على `AuthService` (وهو جزء من الحالة) وتُعيد `Option<TrustedSender>` مع خطأ `AuthError` تُحوّله الطبقات إلى `(StatusCode, String)` كما في `auth_error_with_limit` ✓.
4. **مفتاح التطبيق في `authorize_topic`:** مسار `/topics/subscribe|unsubscribe` معالجُه في `src/topics.rs` (ممنوع) ويستدعي `auth.authorize_topic` في `src/auth.rs` (مسموح)، ففرع مفتاح التطبيق وُضع هناك ليأخذ المفتاح مستوى Admin لتطبيقه. النتيجة: الدالة الواحدة `trusted_sender` هي المدخل الوحيد ✓.
5. **`POST /send` و`GET /campaigns` ...:** `authenticate_send` و`authenticate_admin` استعملا `trusted_sender` فصار فحص المفتاح في مكان واحد بدل تكرار `verify_service_key` ✓.
6. **حدّ `last_used_at` يحدّث بالكتابة الأولى:** الاختبار `last_used_writes_are_throttled` يستعمل trigger على SQLite يعدّ الكتابات (الطابع الزمني بدقة ثانية لا يكشف الفرق) ✓.
7. **اختبار `check_malformed_shapes_are_invalid`:** الأشكال المشوهة (قصير، حرف خارج base64url، بادئة بلا جزء عشوائي) ⇒ `Invalid` ✓؛ أما "التطبيق المضمّن لا يطابق" في answering `WrongApp` (لأن المقارنة الفاشلة تعني أن المفتاح لتطبيق آخر) — وهو ما ينصّ عليه §3، فالفرق عن اسم الاختبار موثّق هنا.
8. **commit إضافي (`T11-4`):** اختبار `malformed_body_unauthenticated_is_401` (شرط المراجع على T10-C) كان يستحق commitه المنفصل؛ لم يُدمج في `T11-3` ليبقى كل commit واحد responsibility واحدة.
9. **مفتاح التطبيق يُقبل في `/send` (وليس `/send/template` فقط):** `POST /send` يمرّ بـ`authenticate_send` ⇒ يقبل المفتاح ✓ (وهو المطلوب في §4).

## 6. ملاحظات خارج النطاق

- **جدول تصنيف مواضع `verify_service_key` (كل المواضع الستة):**

  | الموضع | الملف | قبل | بعد T11 |
  |---|---|---|---|
  | `authenticate_admin` | `src/auth.rs` | مفتاح الخدمة | **يقبل مفتاح التطبيق** لتطبيق الطلب (وإلا فهوية المستخدم) |
  | `authenticate_send` | `src/auth.rs` | مفتاح الخدمة | **يقبل مفتاح التطبيق** (وإلا المفتاح القديم ثم الهوية) |
  | `authenticate_admin_any` | `src/auth.rs` | مفتاح الخدمة | **بلا تغيير**: مسار بلا `app` ⇒ لا مفتاح تطبيق |
  | `authorize_topic` | `src/auth.rs` | مفتاح الخدمة | **يقبل مفتاح التطبيق** بمستوى Admin لتطبيقه |
  | `send_template` | `src/main.rs` | مفتاح الخدمة | **يقبل مفتاح التطبيق** (كما الخدمة: يتخطى `senders` وحد الـuid) |
  | `authorize_campaign_sender` | `src/main.rs` | مفتاح الخدمة | **يقبل مفتاح التطبيق** (كما الخدمة: بلا مراجعة) |

- المسارات التي **لا** تقبل مفتاح التطبيق: `GET /apps` (القديم) و`authenticate_admin_any`، `GET /history`، `GET/POST /topics` (الإداريتان القديمتان) — لأن §4 يقرّر بقاءها للمفتاح العام فقط ولم يجد أن قبول المفتاح فيها ضروري ✓. `POST /add_topic` كذلك (المسار الإداري القديم) ✓.
- `API_KEY` القديم يبقى في `/send` فقط ✓، ومفتاح التطبيق **لا** يُقبل في `/admin/*` ولا `/devices/*` ✓ (اختباران).
- `require` في القوالب التي تعتمد `{sender.uid}`: uid المفتاح = `key:<id>` ⇒ 403، كماقرر المراجع لمفتاح الخدمة (يُوثَّق في T13) ✓.
- (من T10-C/B) متابعات T13: حد `test-send` على `auth_fail_limiter`، حد الإرسال الحر على `topics_limiter`، استخراج `build_router(state)`، اختبار مخصص لـ`dedupe.seconds`، وحدود الكتابة على `devices_limiter` بمفاتيح مميزة.
- (من T10-B) `PUT /apps/{app}/campaigns` هو مسار القسم و`GET /apps/{app}/campaigns` هو القائمة — يجب أن تعكس الواجهة (T12) ذلك.
- (من T9) مخبأ الأدوار يُبطَل عند كل حفظ قالب، الأنماط بلا `size_limit`، `validate_data_source` يقبل `http://localhost.evil.com`، `api_fallback` لا يغطي مسارَي الأجهزة.
- مستودع `taqati`: لم يُمَس (لا commit ولا push).

## 7. أسئلة للمالك/المراجع

1. **مفتاح التطبيق في `/send`:** §4 يعدّه مقبولاً ✓ — أُضيف. هل `/send` (الإرسال الخام) يجب أن يقبل مفتاح التطبيق أصلاً، أم فقط `/send/template`؟
2. **`GET /history` و`/topics` الإداريتان:** بقيا للمفتاح العام فقط (§4) — موافق؟
3. **`hint`:** العمود الوحيد المضاف على 3.14 (آخر 4 أحرف) — موافق على إضافته إلى صيغة الخطة؟ (أُضيف في §3.14 مع تعليق.)
4. **مفتاح التطبيق ومسار `/topics/subscribe`:** استُعمل `authorize_topic` في `src/auth.rs` ليأخذ المفتاح مستوى Admin لتطبيقه؛ هل تفضَّل البقاء هكذا أم منعه هناك؟
5. **لا حقل `AppState`:** استُعمل `AuthService` حاملاً للمخزن لأن `src/devices.rs` ممنوع ويبني `AppState` — هل يُسمح بتعديل `src/devices.rs` سطراً واحداً لاحقاً ليأخذ الحقل شكل الخطوة؟
6. **الخطوة التالية بعد T11: T12** (واجهة الداشبورد) أو T16 (إضافة تطبيق من الداشبورد)؟

## 8. حالة git النهائية

```
$ git status --short
?? docs/plans/execution/T11.md
(بعد هذا الـ commit يصبح كل شيء نظيفاً)

$ git log --oneline -7
<آخر commit>  docs(plan): T11 update execution tracker
1599bd2 test(admin): T11-4 cover the session before the body
ff671fb feat(admin): T11-3 add app key management routes
a4c0764 feat(auth): T11-2 accept app keys on app-scoped send routes
8f21d5f feat(appkeys): T11-1 add app_keys table and AppKeyStore
fc4f12c fix(admin): T11-0 remove expect from the diagnostics production path
9c8d3d2 docs(plan): T10C update execution tracker
```

- بلا `git push` (لم يُنفَّذ إطلاقاً). بلا `git reset --hard`/`git clean`/`git checkout -- .`/`git stash drop`. stash الموجود (`stash@{0}` بتاريخ 2026-09-23) لم يُنشأ ولم يُمَس.
- لا قيم أسرار في أي commit أو في هذا التقرير؛ الإشارة إلى الأسرار بالمسار فقط. (المفاتيح في الاختبارات مولّدة لكل اختبار، ولا تُطبع في أي ردّ ولا في أي assertion.)

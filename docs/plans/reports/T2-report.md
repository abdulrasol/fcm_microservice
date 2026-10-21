# تقرير T2

- التاريخ/الوقت: 2026-10-21 11:20 +03
- الفرع و HEAD قبل/بعد: `feature/policy-auth` — قبل: `2213d09` — بعد كود الجزأين: `bc42dc0` (يليه commit الوثائق الذي يحمل هذا الملف)
- النطاق المنفَّذ: الجزء 0 (F1–F4 + 7 اختبارات) ثم الجزء 1 (حسابات الداشبورد والجلسات + 34 اختباراً)

## 1. ما نُفِّذ (بند بند من ملف الخطوة)

| البند | الحالة | ملاحظة |
|---|---|---|
| F1 — قفل الكتابات في `ConfigStore` | تم | `write_lock: tokio::sync::Mutex<()>`، ويُؤخذ في أول سطر من الدوال السبع (`create_app`, `save_section`, `save_template`, `set_template_enabled`, `delete_template`, `restore`, `import`) ويُبقى حتى بعد `update_cache` |
| F1 — دوال داخلية `*_locked` بلا قفل مزدوج | تم | `create_app_locked`, `save_section_locked`, `save_template_locked`, `set_template_enabled_locked`, `delete_template_locked`, `restore_locked`, `import_locked`؛ `restore_locked` ينادي الدوال الداخلية لا العامة |
| F1 — قراءة `snapshot` بعد القفل | تم | أول سطر في كل دالة هو `let _guard = self.write_lock.lock().await;` ثم التحقق ثم قراءة اللقطة |
| F1 — فحص `expected_version` تحت القفل | تم | `template_version` وقراءة اللقطة كلاهما بعد القفل |
| F1 — تعليق افتراض النسخة الواحدة | تم | أعلى `src/config/store.rs` (سطور 1–8) |
| F1 — قرار عدم رفع `app_configs.version` عند حفظ قالب + تعليق على `AppSnapshot::version` | تم | التعليق على الحقل في `store.rs` |
| F2 — `save_template` بـ UPSERT يحفظ `last_used_at` | تم | `INSERT ... ON CONFLICT(app, key) DO UPDATE SET definition/enabled/version/updated_by/updated_at` |
| F2 — `updated_at = CURRENT_TIMESTAMP` في `write_config` و`set_template_enabled` | تم | كلاهما محدَّث |
| F2 — `import` يحذف القوالب الغائبة فقط ويحدّث الباقي بـ UPSERT | تم | حُذف `DELETE FROM templates WHERE app = ?` الشامل |
| F3 — حاجز `[G6]` | تم | في `validate_non_admin_guards` (صار يستقبل `cfg`)، الرسالة حرفياً كما في الطلب |
| F4 — ترجمة العربية في `src/config/*.rs` و`src/cli.rs` وتعليقات T1 في `db.rs`/`main.rs` | تم | 131 سطراً استُبدل (تفصيله في §3)؛ لا تغيير منطق |
| اختبارات الجزء 0 السبعة | تم | كلها موجودة وناجحة (الأسماء في §3) |
| commit الجزء 0 مستقل أولاً | تم | `bc64304 fix(config): T2-0 ...` |
| اختبار «يفشل عند تعطيل القفل» يدوياً مرة واحدة | تم | تم تعطيل القفل مؤقتاً: `left: 1, right: 8` — التفاصيل في §4 |
| §1 الاعتماديات | تم | `argon2 = "0.5"`، نقل `rand = "0.8"` و`base64 = "0.22"` إلى `[dependencies]`، وdev: `tower 0.4 util` + `http-body-util 0.1`. لا شيء غير ذلك |
| §2 الجداول الثلاثة + الفهرس | تم | `tool_users`, `tool_user_apps`, `tool_sessions`, `idx_tool_sessions_user` |
| §2 `PRAGMA foreign_keys = ON` لكل اتصال (وفي قاعدة الاختبار) | تم | عبر `SqliteConnectOptions::foreign_keys(true)` في `init_db` وفي `memory_pool` في الاختبارات؛ مُختبَر بحذف مستخدم |
| §2 نوع أوقات الصفوف | تم كـ**انحراف مبرَّر** | `INTEGER` epoch seconds (مسموح صراحةً في الخطوة) — التفاصيل في §5 |
| §3 argon2id + spawn_blocking + سياسة 12..=128 + قائمة 20 | تم | `password.rs` |
| §3 `verify_or_dummy` بهاش وهمي يُولَّد عند الإقلاع | تم | `ToolAuth::new` يولّد `dummy_hash` مرة واحدة؛ يُستخدم عند البريد غير الموجود |
| §4 توكن 32 بايت base64url + تخزين SHA-256 hex فقط | تم | `session.rs` |
| §4 صلاحية 12 ساعة و`ADMIN_SESSION_HOURS` بسقف 72 | تم | `CookieConfig::from_env` + `with_hours` (clamp) |
| §4 حد 10 جلسات وحذف الأقدم | تم | `create_session` (يحذف المنتهية ثم الأقدم) |
| §4 تنظيف المنتهية عند الدخول + مهمة كل ساعة | تم | `create_session` + `spawn_session_cleanup` |
| §4 الكوكي `__Host-fcm_session` / `fcm_session` + الخصائص + لا Domain | تم | `set_cookie` / `clear_cookie` |
| §4 تحليل `Cookie` وتركيب `Set-Cookie` يدوياً + اختبارات | تم | `read_cookie` مع اختبارات المسافات/الترويسات المتعددة |
| §4 `COOKIE_SECURE` و`ADMIN_SESSION_HOURS` في `.env.example` بالإنجليزية | تم | قسم `--- Dashboard sessions (T2) ---` |
| §5 المسارات الخمسة تحت `/api/v1/admin` | تم | `.nest("/admin", admin::router(state.clone()))` داخل `api_routes` قبل `.with_state`، ويرث `RequestBodyLimitLayer` و`api_fallback` |
| §5 غلاف الأخطاء JSON والأكواد الخمسة | تم | `AdminError` + `AdminJson` (حتى أخطاء جسم الطلب بصيغة JSON)؛ أضيف كود سادس `internal` لـ500 — §5 |
| §5 حماية CSRF (ترويسة + Origin) على كل غير GET/HEAD/OPTIONS بما فيه login | تم | `csrf_guard` كطبقة على الـ router |
| §5 بوابة `must_change_password` | تم | في `AuthedUser::from_request_parts` عبر `requires_password_change` |
| §5 `AuthedUser` كـ`FromRequestParts<Arc<AppState>>` بالحقول السبعة | تم | `auth.rs` |
| §5 `require_owner` / `require_app` / `require_reauth` + اختبارها | تم | `#[allow(dead_code)]` موثّق لأن T10 هي التي تربطها بالمسارات |
| §5 حدود المحاولات 5/15د للبريد وIP، والسادسة 429 بلا فحص كلمة | تم | `is_blocked` مضاف في `ratelimit.rs` (بلا تغيير سلوك `check`)، ويُفحص قبل أي تجزئة |
| §5 نجاح الدخول لا يصفّر العداد | تم | `check` يُستدعى عند الفشل فقط |
| §5 `ip`/`user_agent` في الجلسة | تم | `client_ip(headers, Some(peer), trust_proxy)` + `User-Agent` مقصوص 200 |
| §5 تدقيق الصفوف الخمسة بلا أسرار | تم | `admin.login` / `admin.login_failed` (hash قصير أو `unknown_email`) / `admin.logout` / `admin.password_changed` / `admin.reauth` |
| §6 `create-owner` | تم | `TEMP_PASSWORD=...` مرة واحدة، مؤقتة 20 حرفاً بلا `0O1lI`، `must_change_password=1`، تدقيق `actor="cli"`، تكرار ⇒ كود 1 بلا تعديل |
| §6 `reset-owner-password` | تم | كلمة جديدة + `must_change_password=1` + حذف كل الجلسات + كود 1 لغير المالك/غير الموجود |
| §6 تحقق البريد المبسّط + lower-case | تم | `normalize_email` |
| §6 لا طباعة كلمة المرور إلا في stdout الأمرين | تم | لا `tracing` لكلمة المرور في أي مكان |
| §7 الاختبارات المطلوبة (32 اسماً) | تم | كلها موجودة بلا `#[ignore]` (§3) |
| §8 الجودة + اليدوي | تم | §3 و§4 |
| §9 الـ commits الثلاثة | تم | `bc64304`، `bc42dc0`، ثم commit الوثائق |
| عدم تعديل `dashboard/` والملفات الممنوعة | تم | §4 البند 1 |

## 2. الملفات (git diff 2213d09..HEAD --stat)

```text
 .env.example              |    7 +
 Cargo.toml                |    7 +-
 src/admin/auth.rs         |  516 ++++++++++++++
 src/admin/mod.rs          | 1069 +++++++++++++++++++++++++++++++++++++++++++++
 src/admin/password.rs     |  190 ++++++++
 src/admin/session.rs      |  359 +++++++++++++++
 src/cli.rs                |  378 +++++++++++++-
 src/config/mod.rs         |   36 +-
 src/config/placeholder.rs |   54 +--
 src/config/store.rs       |  445 ++++++++++++++++---
 src/config/validate.rs    |  167 ++++---
 src/db.rs                 |   70 ++-
 src/main.rs               |   16 +-
 src/ratelimit.rs          |   48 ++
 14 files changed, 3202 insertions(+), 160 deletions(-)
```

ملفات جديدة وسطر شرح لكل منها:

| الملف | الأسطر | الشرح |
|---|---|---|
| `src/admin/mod.rs` | 1069 | `ToolAuth`، غلاف `AdminError` بصيغة JSON، مستخرج `AdminJson`، طبقة `csrf_guard`، بناء الـ router، مهمة تنظيف الجلسات، و20 اختبار HTTP على قاعدة ذاكرة |
| `src/admin/auth.rs` | 516 | `Role` و`ToolUser` و`UserView`، التحميل من القاعدة، `AuthedUser` والـ extractor، معالجات login/logout/me/password/reauth، وحرّاس `require_*` |
| `src/admin/password.rs` | 190 | argon2id على `spawn_blocking`، سياسة كلمة المرور وقائمة الشائع، `verify_or_dummy`، 4 اختبارات |
| `src/admin/session.rs` | 359 | توليد التوكن وتجزئته، إعداد الكوكي، إنشاء/تحميل/حذف الجلسات، تنظيف المنتهية، 5 اختبارات |

الملفات المعدَّلة (وصف موجز):

| الملف | التغيير |
|---|---|
| `src/config/store.rs` | F1 + F2 + ترجمة + 5 اختبارات جديدة |
| `src/config/validate.rs` | `[G6]` + تمرير `cfg` + ترجمة + اختباران |
| `src/config/mod.rs`, `placeholder.rs` | ترجمة تعليقات فقط (F4) |
| `src/cli.rs` | `create-owner` و`reset-owner-password` + ترجمة + 4 اختبارات |
| `src/db.rs` | 3 جداول + فهرس + `foreign_keys(true)` + `write_audit` + ترجمة تعليق |
| `src/main.rs` | `mod admin`، `tool_auth` في `AppState`، `.nest("/admin", ...)`، تشغيل تنظيف الجلسات، ترجمة تعليق |
| `src/ratelimit.rs` | `is_blocked`/`is_blocked_at` + اختبار |
| `Cargo.toml` | الاعتماديات المذكورة في §1 فقط |
| `.env.example` | `COOKIE_SECURE` و`ADMIN_SESSION_HOURS` |

## 3. الأوامر التي شغلتها ونتائجها

الجزء 0 (بعد `bc64304`):

```text
$ cargo build
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.59s

$ cargo test
test result: ok. 84 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.20s
```

(84 = 77 القديمة + 7 الجديدة، وكل اختبارات T1 ما زالت ناجحة.)

الجزء 1 (بعد `bc42dc0`):

```text
$ cargo build
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.94s

$ cargo clippy --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.92s
$ cargo clippy --all-targets 2>&1 | grep -c "^warning"
0

$ cargo test
test result: ok. 118 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 17.11s

$ cargo test -- --list | grep -c ": test$"
118
```

الأسماء الجديدة كما في `cargo test -- --list` (41 اختباراً جديداً: 7 للجزء 0 + 32 المطلوبة + 2 إضافيان):

```text
admin :: audit_rows_written_without_secrets
admin :: change_password_clears_flag_and_kills_other_sessions
admin :: change_password_policy_violation_is_400
admin :: change_password_wrong_current_is_401
admin :: deleting_user_cascades_sessions
admin :: eleventh_session_evicts_oldest
admin :: expired_session_is_rejected_and_cleaned
admin :: login_disabled_user_returns_401
admin :: login_success_sets_cookie_and_returns_user_without_hash
admin :: login_with_foreign_origin_is_403
admin :: login_without_csrf_header_is_403
admin :: login_wrong_password_and_unknown_email_return_identical_401
admin :: logout_deletes_session_and_clears_cookie
admin :: me_requires_session
admin :: must_change_password_blocks_other_routes
admin :: reauth_sets_timestamp
admin :: require_app_manager_scope
admin :: require_owner_rejects_manager
admin :: require_reauth_rejects_stale_and_missing
admin :: sixth_failed_login_returns_429_without_checking_password
admin::password :: hash_then_verify_roundtrip
admin::password :: password_policy_rejects_short_and_email_equal_and_common
admin::password :: unknown_email_still_runs_dummy_verify
admin::password :: verify_rejects_wrong_password
admin::session :: cookie_parse_handles_multiple_and_spaces
admin::session :: session_id_is_32_random_bytes_and_stored_hashed
admin::session :: set_cookie_has_required_attributes_secure_mode
admin::session :: set_cookie_insecure_mode_uses_plain_name
admin::session :: hours_are_clamped_to_the_allowed_range            (إضافي)
cli :: create_owner_existing_email_fails_without_changes
cli :: create_owner_inserts_owner_with_temp_flag
cli :: reset_owner_password_kills_sessions
cli :: reset_owner_password_rejects_manager
config::store :: concurrent_section_and_template_saves_leave_cache_equal_to_db
config::store :: concurrent_template_saves_keep_all_templates_in_cache
config::store :: import_removes_templates_missing_from_document
config::store :: save_section_refreshes_updated_at
config::store :: template_save_preserves_last_used_at
config::validate :: g6_admin_role_target_allowed
config::validate :: g6_non_admin_role_target_must_be_admin_role
ratelimit :: is_blocked_reports_without_recording_a_hit             (إضافي)
```

عدد أسطر العربية المستبدلة في F4: 131 سطراً (`config/mod.rs` 18، `placeholder.rs` 27، `validate.rs` 51، `store.rs` 27، `cli.rs` 6، `db.rs` 1، `main.rs` 1 — بعض أسطر الاختبارات كانت بيانات نصية عربية داخل `json!` فتُرجمت أيضاً).

التحقق اليدوي (قاعدة `t2_demo.db` مؤقتة حُذفت بعده؛ قيمة الكوكي مستبدلة بـ`<redacted>` وكلمة المرور المؤقتة لم تُطبع):

```text
create-owner exit: 0
stdout format: TEMP_PASSWORD=<redacted>
temporary password length: 20
server ready: yes
--- login ---
HTTP/1.1 200 OK
content-type: application/json
set-cookie: fcm_session=<redacted>; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200
{"user":{"apps":[],"email":"owner@example.com","id":1,"must_change_password":true,"role":"owner"}}
--- me (before the password change) ---
HTTP/1.1 200 OK
content-type: application/json
{"apps":[],"email":"owner@example.com","id":1,"must_change_password":true,"role":"owner"}
--- reauth while must_change_password ---
status=403
--- password change ---
HTTP/1.1 204 No Content
--- me (after the password change) ---
HTTP/1.1 200 OK
content-type: application/json
{"apps":[],"email":"owner@example.com","id":1,"must_change_password":false,"role":"owner"}
--- mutating request without X-Requested-With ---
status=403
--- logout ---
HTTP/1.1 204 No Content
set-cookie: fcm_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0
--- me after logout ---
status=401
--- git status (db files) ---
no *.db in git status
```

ملاحظة: الاسم `fcm_session` (لا `__Host-`) لأن التحقق اليدوي شُغّل بـ`COOKIE_SECURE=false` كما طلب البند 2؛ اختبار `set_cookie_has_required_attributes_secure_mode` يغطي الوضع الآمن.

## 4. طريقة الفحص

**1) نطاق التغييرات:** `git diff 2213d09..HEAD --stat` = 14 ملفاً (§2)، كلها ضمن قائمة §1 (بما فيها `src/ratelimit.rs` المسموح لـ`is_blocked`) زائد ملفات الجزء 0 التي يسميها F4 (`src/config/*.rs`, `src/cli.rs`, `src/db.rs`, `src/main.rs`). والملفات الممنوعة:

```text
$ git diff --stat 2213d09 -- src/auth.rs src/topics.rs src/validation.rs src/credentials.rs dashboard
(فارغ: لا تغيير)
```

**2) العربية في `src/`:** `grep -rnP` غير مدعوم في `grep` على macOS (`grep: invalid option -- P`)، فاستخدمت مكافئاً بـ`perl -CSD` على كل ملفات `src`:

```text
$ for f in $(find src -name '*.rs'); do perl -CSD -ne '...' $f; done
src/validation.rs: 1 سطر
src/topics.rs: 1 سطر
```

السطران هما **بيانات اختبار** لا تعليقات، داخل ملفين ممنوع تعديلهما:
- `src/validation.rs:198` → `for topic in ["/topics/all", "a b", "عربي", "a/b", "a\nb"]`
- `src/topics.rs:229` → `for name in ["", "/topics/all", "a b", "عربي", "a/b"]`

بقية `src/` (ومنها `src/config/*`، `cli.rs`, `db.rs`, `main.rs`, `admin/*`, `auth.rs`, `credentials.rs`) صفر عربية. هذا يعني أن معيار القبول 2 يعطي **نتيجتين** لا صفراً، والسبب تعارض بين F4 (يحصر الترجمة في ملفات محددة) و§1 (يمنع هذين الملفين) — انظر §6/§7، ولم أصلحه التزاماً بقاعدة «لا تصلح ما هو خارج النطاق».

**3) أسماء الاختبارات وعدم التعطيل:** الـ41 اسماً كلها موجودة عبر `cargo test -- --list` (§3)، و`grep -c "#\[ignore\]"` على `src/**/*.rs` = 0، واختبارات T1 القديمة (77) كلها ناجحة (`118 passed; 0 failed`).

**4) القفل:**

```text
$ grep -n "write_lock" src/config/store.rs
7:  //! Single-instance assumption: `write_lock` serializes writers ...
111:    write_lock: tokio::sync::Mutex<()>,
178:            write_lock: tokio::sync::Mutex::new(()),
202:        let _guard = self.write_lock.lock().await;
272:        let _guard = self.write_lock.lock().await;
363:        let _guard = self.write_lock.lock().await;
468:        let _guard = self.write_lock.lock().await;
547:        let _guard = self.write_lock.lock().await;
608:        let _guard = self.write_lock.lock().await;
709:        let _guard = self.write_lock.lock().await;
$ grep -c "INSERT OR REPLACE INTO templates" src/config/store.rs
0
```

سبعة أخذ للقفل = الدوال السبع، وكلها في أول سطر من الدالة (بعد التوقيع)، وتشمل `restore` و`import`، و`restore_locked` ينادي `save_*_locked` فلا قفل مزدوج. `INSERT OR REPLACE` بقي على `app_configs` فقط (كما سمحت الخطوة).

**5) لا `unwrap()`/`expect(` خارج الاختبارات في `src/admin` و`src/cli.rs`:** سكربت يقرأ كل ملف ويقف عند أول `#[cfg(test)]`:

```text
(لا نتائج) — checks done
```

أي أن كل `unwrap()`/`expect(` في هذين الموضعين داخل وحدات الاختبار فقط.

**6) لا أسرار:** لا كلمة مرور مؤقتة ولا قيمة كوكي في التقرير أو في الـ commits (التحقق اليدوي يطبع `<redacted>`)، ولا `.env` ولا `credentials/` ولا `data.db` معدَّلة (`git status` بلا `*.db`). كلمة المرور الوهمية في `ToolAuth` تُولَّد عند الإقلاع ولا تُطبع.

**7) تجربة تعطيل القفل (الجزء 0، مرة واحدة):** عُطّل سطر `let _guard = ...` في `save_template` مؤقتاً ثم:

```text
$ cargo test concurrent_template_saves_keep_all_templates_in_cache
assertion `left == right` failed: no concurrent save may be lost from the cache
  left: 1
 right: 8
failures:
    config::store::tests::concurrent_template_saves_keep_all_templates_in_cache
test result: FAILED. 0 passed; 1 failed; ...
```

أي أن الكاش احتفظ بقالب واحد من ثمانية (بالضبط العطل الذي وصفته المراجعة). أُعيد الملف من نسخة احتياطية فوراً (`grep -c "LOCK DISABLED"` = 0) والاختبار ناجح الآن.

**8) الاختبار اليدوي:** §3 — تسلسل login ⇒ me(true) ⇒ reauth مرفوض 403 ⇒ password 204 ⇒ me(false) ⇒ طلب بلا ترويسة CSRF 403 ⇒ logout 204 + حذف الكوكي ⇒ me 401.

**9) فحص بنيوي إضافي (تشغيل حقيقي على قاعدة مؤقتة ثانية `t2_probe.db` ثم حُذفت):** للتأكد من شرط §5 «يرث `RequestBodyLimitLayer` ويمر `api_fallback`»:

```text
--- unknown admin path (expect api_fallback 404 JSON) ---
status=404 {"available_endpoints":[{"description":"Send FCM push notification ...
--- body over 64KB on login (expect 413) ---
status=413
--- existing endpoint still plain text (no CSRF there) ---
status=422
```

المسار `/api/v1/send` (422 من تحقق المعالج) يثبت أن طبقة CSRF محصورة في `/api/v1/admin` ولم تمسّ بقية الأداة. والشجرة بقيت نظيفة بعد الحذف.

## 5. انحرافات عن الأمر (وسببها)

1. **نوع أعمدة الوقت `INTEGER` epoch** في `tool_sessions` (`expires_at`, `reauth_at`, `created_at`) بدل `DATETIME`. مسموح صراحةً في §2 («إذا اخترت INTEGER فغيّر أنواع الأعمدة… وسجّل ذلك كانحراف مبرَّر»). السبب: مقارنات صحيحة بلا أخطاء نصية/مناطق زمنية، و`reauth_at` يُقارن حسابياً في `require_reauth`. بقية الجداول أبقَت `DATETIME DEFAULT CURRENT_TIMESTAMP` كما في §2 (لا تُقارن نصياً في الكود).
2. **كود خطأ سادس `internal` (500)** غير مذكور في قائمة §5. لزم لأن أخطاء القاعدة يجب أن تُرجع غلافاً JSON أيضاً؛ الرسائل لا تكشف تفاصيل القاعدة (تُسجَّل بـ`tracing` وحدها). الأكواد الخمسة المطلوبة مستخدمة كما هي.
3. **`AdminJson` مستخرج مخصص** ليردّ `400 {"error":"invalid_request"}` بدل نص axum الافتراضي، تنفيذاً لجملة §5 «أخطاء هذا الـ Router بصيغة JSON موحدة».
4. **اختباران إضافيان** فوق المطلوب: `hours_are_clamped_to_the_allowed_range` (سقف 72 والأرضية 1) و`is_blocked_reports_without_recording_a_hit` (لأن `is_blocked` تعديل مسموح في `ratelimit.rs` وطُلب اختباره). المجموع 41 لا 39.
5. **`NewSession` يعيد `token` فقط** (بلا `id_hash`/`expires_at`) بعد إزالة حقول لم يقرأها أحد، لتفادي تحذيرات dead-code. و`delete_user_sessions` كُتبت ثم حُذفت لأن موضعي الحذف (تغيير كلمة المرور، إعادة تعيين المالك) يحتاجان الحذف **داخل** المعاملة، والدالة تأخذ `&SqlitePool` لا معاملة.
6. **`#[allow(dead_code)]` على حرّاس `require_owner`/`require_app`/`require_reauth` وحقل `reauth_at`** لأن §5 يطلب إضافتها «ولا تربطها بمسارات الآن»؛ بدونه يفشل شرط «صفر تحذيرات». موثّق بتعليق يذكر أن T10 هي التي تربطها.
7. **`rustfmt` على ملفاتي فقط**، لكن `rustfmt src/main.rs` يتبع شجرة الوحدات فنسّق `src/credentials.rs` (وهو غير منسّق أصلاً في HEAD). استعدت الملف بايت-ببايت من `git show HEAD:src/credentials.rs` وتحققت أن `git diff` عليه فارغ، ثم استخدمت `rustfmt` على ملفات الوحدات مباشرة. لم أستخدم `git checkout -- .`.
8. **رسائل الـ commits** أضفت إليها سطر `Refs: docs/plans/secure-sending-plan.md T2` وجسماً قصيراً، التزاماً بـ RULES §3 (كان ملف الخطوة يقترح سطر العنوان فقط). أُضيف السطر إلى commit الجزء 0 أيضاً بإعادة كتابة محلية لـ commitَيّ فقط (بلا push، والشجرة بعد إعادة الكتابة مطابقة تماماً للشجرة المُختبَرة: `git rev-parse HEAD^{tree}` متساوٍ قبل/بعد).
9. **`src/db.rs`**: أضفت `pub async fn write_audit(...)` بدل كتابة SQL مكرر في `admin` و`cli`؛ الملف مسموح، والدالة صغيرة وتُستخدم في الموضعين.

## 6. ملاحظات خارج النطاق (لم أصلحها)

1. **CORS لا يسمح بترويسة `X-Requested-With`.** طبقة CORS الحالية في `src/main.rs` تسمح بـ`AUTHORIZATION`, `CONTENT_TYPE`, `ACCEPT` فقط. المتصفح سيفشل في preflight لأي طلب `POST` من الداشبورد قبل وصوله إلى `csrf_guard` (أي أن T12 لن تستطيع تسجيل الدخول من متصفح حقيقي حتى تُضاف الترويسة إلى `allow_headers`). `curl` لا يفعل preflight فمرّ التحقق اليدوي. لم أمسّ الطبقة لأن T2 §1 لم يذكرها؛ أرى أن مكانها T12 أو إصلاح صغير قبلها.
2. **عربية في ملفين ممنوعين:** `src/validation.rs:198` و`src/topics.rs:229` (بيانات اختبار `"عربي"`). هذا يجعل معيار القبول 2 يعطي نتيجتين. الحلان الممكنان: (أ) تغيير العينة إلى نص غير عربي (مثل `"日本語"`) وهو تعديل سطر واحد في ملفين ممنوعين، (ب) قبول الاستثناء وتوثيقه. لم أنفّذ أياً منهما.
3. **`PUBLISHED_COMMIT` في `docs/plans/reports/S0-report.md` ما زال `<يملؤه المالك>`**، والـ hash المذكور `c303d3` غير موجود في أي من المستودعين.
4. **فرع `feature/policy-notifications`** المذكور في S0-4 لم يُنشأ (S0 B2 طلب `feature/sec2c-id-token` فقط) — سؤال مالك من S0 ما زال مفتوحاً.
5. **محدّد المحاولات في الذاكرة لكل عملية:** إعادة تشغيل الخادم تصفّر عداد 5/15د، وعلى أكثر من نسخة (multi-instance) لا يُشارَك العداد. مقبول للنسخة الواحدة (نفس افتراض `ConfigStore`)، ويحتاج تخزيناً مشتركاً إن تعددت النسخ.
6. **`ToolAuth::new` يقرأ `CORS_ALLOWED_ORIGINS` مرة ثانية** (لقائمة `Origin` المسموحة في CSRF). إن اختلف المصدران مستقبلاً (مثلاً قراءة من ملف) يمكن أن يمر الطلب من CSRF ويفشل في CORS؛ حالياً المتغير واحد فلا تعارض.
7. **حذف الجلسة المنتهية يحدث كتابةً على `GET /auth/me`** (كل تحميل جلسة منتهية يحذف صفها). هذا تنظيف متعمَّد (fail-closed + جدول صغير)، لكنه كتابة على مسار قراءة.
8. **`tool_users.test_uid`** موجود في الجدول ولا كود يستخدمه بعد (T10/T11).
9. **`import` يعيد رقم إصدار كل قالب إلى 1** (سلوك T1 محفوظ حرفياً بنفس الـUPSERT). لم أغيّره لأن F2 طلبت «نفس الـ UPSERT»؛ لكنه يعني أن استيراداً كاملاً يمسح تاريخ إصدارات القوالب (التاريخ يبقى في `config_versions`).

## 7. أسئلة للمالك/المراجع

1. **CORS:** هل أضيف `x-requested-with` إلى `allow_headers` الآن (تعديل سطر واحد في `main.rs`) قبل T12، أم أتركها لـ T12؟
2. **العربية في الملفين الممنوعين:** أستبدل `"عربي"` بعينة غير عربية (سطر واحد في كل ملف)، أم تُعدَّل صيغة معيار القبول 2 لتستثني بيانات الاختبار في ملفات ممنوعة؟
3. **`import` وتاريخ إصدارات القوالب:** هل يجب أن يرفع إصدار كل قالب موجود (`version = version + 1`) بدل إرجاعه إلى 1؟ القرار الحالي = 1 حفاظاً على سلوك T1.
4. **كود 500:** أُبقي `internal` كما هو، أم تفضّل اسماً ضمن عائلة الأكواد (`server_error`) قبل أن تعتمد عليه T10؟
5. **`reauth`:** حالياً يضبط `reauth_at` فقط ولا يمدّد عمر الجلسة (`expires_at`). هل المطلوب أن يمدّدها في T10؟
6. **S0:** `PUBLISHED_COMMIT` ما زال فارغاً في تقرير S0 — هل أملؤه بـ`14fd68a` (آخر commit على `master`) أم بغيره؟

## 8. حالة git النهائية

قبل commit الوثائق:

```text
$ git status --short
?? docs/plans/execution/T2.md        (سيُضاف في commit الوثائق)

$ git log --oneline -5
bc42dc0 feat(admin): T2 add dashboard accounts, sessions, password and reauth
bc64304 fix(config): T2-0 serialize config writes, keep last_used_at, add G6, English comments
2213d09 docs(plan): T1 update execution tracker and DDL
010a879 feat(config): T1 add app config model, validation and SQLite store
8a08046 docs(plan): S0 add secure sending plan, execution rules and deploy warning
```

وبعد commit الوثائق (`docs(plan): T2 update execution tracker`، يضم هذا التقرير و`T2.md` وتحديث `execution/README.md` وسجل التقدم في الخطة) تكون الشجرة نظيفة: `git status --short` بلا مخرجات، بلا push، وبلا أي `*.db` جديد.

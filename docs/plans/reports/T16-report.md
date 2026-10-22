# تقرير T16 (الجزء الخلفي)

- **التاريخ/الوقت:** 2026-10-22، من 05:00 إلى 09:00 (توقيت الجهاز).
- **الفرع:** `feature/policy-auth` (تحقّق قبل البدء وقبل كل commit؛ لم يتغيّر).
- **HEAD قبل العمل:** `2bf54f8` (`docs(plan): T11 update execution tracker`).
- **HEAD بعد العمل:** آخر commit هو `docs(plan): T16 update execution tracker` — 5 commits جديدة (4 كود + 1 توثيق)، بلا push.
- **ملف الخطوة المنفَّذ:** `docs/plans/execution/T16.md` (الخلفية فقط؛ شاشة `AddApp.vue` ضمن T12).

## 1. ما نُفِّذ (بند بند من ملف الخطوة)

| البند | الحالة | ملاحظة |
|---|---|---|
| §0 التعديلات المسموحة | تم | جديد: `src/admin/credentials.rs`. تعديل: `src/credentials.rs` (المدير + التحقق + التثبيت)، `src/admin/mod.rs` (المسارات + `test_state()` + المصفوفة)، `src/admin/apps.rs` (علامة `disabled` فقط)، `src/db.rs` (جدولان + دوال)، `src/main.rs` (تهيئة المدير والمتغيرات)، `src/config/store.rs` (**إضافة `delete_app` فقط**، +178/−0)، `src/devices.rs` (**سطور بناء `AppState` داخل اختبار المساعد فقط** ✓ كما سمحت الخطوة)، `.env.example`، `docs/plans/**`. ممنوع: بقية `src/` و`Cargo.toml` و`dashboard/` و`Dockerfile` — لم يُمَس. |
| §1 المدير `CredentialManager` | تم | `src/credentials.rs`: البنية كما في الخطوة (`dir`, `auth_map`, `project_map`, `disabled`, `verifier`, `lock`, `enabled`). `CredentialVerifier` + `GoogleVerifier` (طلب access token بنطاق `firebase.messaging` ومهلة 15 ثانية). `ValidatedKey` (+ `Debug` يطبع المعرّفات فقط). |
| §1 `parse_and_validate` | تم | حجم ≤ 8192 بايت، JSON كائن، `type == "service_account"`، الحقول الأربعة نصوص غير فارغة، `project_id` `[a-z0-9-]{1,100}`، `client_email` ينتهي بـ`.gserviceaccount.com`، المفتاح يبدأ بـ`-----BEGIN PRIVATE KEY-----`، وبناء `ServiceAccountKey` يمرّ (PEM تالف ⇒ `Invalid("the private key could not be parsed")`). **لا قيمة من الملف في أي رسالة خطأ** (اختبار `errors_never_contain_key_material`). |
| §1 `app_name_ok` | تم | `^[a-z0-9_-]{1,40}$`؛ يُفحص قبل أي عملية ملفات، والمسار دائماً `dir.join(format!("{app}.json"))`. اختبار: `app_name_whitelist_rejects_traversal_and_oddities` (`../x`، `a/b`، `.hidden`، `CON`، مسافة، فارغ، 41 حرفاً ✗؛ `watt`، `my_app_1`، `a`، 40 حرفاً ✓). |
| §1 `install` | تم | الترتيب: (1) `enabled` وإلا `Disabled`؛ (2) `app_name_ok` ثم `parse_and_validate`؛ (3) `replace=false` ⇒ `Exists`، `replace=true` ⇒ يجب أن يكون موجوداً وأن يكون `project_id` الجديد **يساوي** القديم (من `project_map` أو من الملف) وإلا `ProjectMismatch`؛ (4) `verifier.verify` ⇒ `LiveCheckFailed` (السبب يُسجَّل، لا يُجاب)؛ (5) كتابة ذرية: ملف مؤقت `.{app}.upload-{random}` (**لا ينتهي بـ`.json`**) في **نفس المجلد** بـ`create_new` وصلاحيات `0o600` من لحظة الإنشاء، `write_all` + `sync_all` ثم `rename` ثم `sync` للمجلد، وأي فشل يحذف المؤقت؛ (6) تحميل فوري بـ`load_credential` والتحقق من `project_map[app]`، وفشل التحميل يعيد الملف القديم (محفوظاً في الذاكرة) أو يحذف الجديد؛ (7) `InstallOutcome{client_email, private_key_id, project_id, replaced}` — **لا شيء من المفتاح**. |
| §1 `disable` / `enable` / `remove` | تم | التعطيل يضيف للمجموعة ويزيل من `auth_map`/`project_map` فوراً (الملف يبقى)؛ التفعيل يزيلها من المجموعة ويحمّل الملف؛ الحذف يحذف الملف ويزيل من الخرائط والمجموعة. |
| §1 الحالة الدائمة | تم | جدول `app_disabled` + `app_credentials` (§2)؛ التعطيل محفوظ في القاعدة ويُحمَّل عند الإقلاع **قبل** `scan_credentials_dir`، و`load_credential` يتجاهل أي تطبيق معطّل (حتى من المراقب) ✓ اختبار `disabled_app_not_reloaded_by_scan_or_watcher`. |
| §1 `metadata` | تم | يقرأ الملف ويخرج `project_id` و`client_email` و`private_key_id` فقط. |
| §1 المراقب والمسح | تم | `scan_credentials_dir` والمراقب يتجاهلان أي ملف يبدأ بنقطة وأي اسم لا يحقق `app_name_ok` (تحذير بالاسم فقط)؛ **سلوك الملفات الصالحة الحالية لم يتغيّر** (`watt.json` ما زال يُحمَّل ✓ `scan_still_loads_existing_valid_files`). |
| §1 المتغيرات | تم | `CREDENTIALS_DIR` (افتراضي `credentials`) و`CREDENTIALS_UPLOAD_ENABLED` (افتراضي `true`) في `main.rs` فقط؛ `enabled=false` ⇒ كل مسارات هذه الخطوة ترجع 404 بعد `require_owner` (والمسار يبقى في جدول المسارات فتبقى المصفوفة سليمة) ✓ `credentials_upload_disabled_flag_returns_404`. |
| §2 قاعدة البيانات | تم | `app_disabled(app, disabled_by, disabled_at)` و`app_credentials(app, uploaded_by, uploaded_at)` في `migrate` (idempotent)، وكل الدوال بـ`?` للـbind. |
| §3 المسارات | تم | `POST /apps` (201 `{app, project_id, client_email, private_key_id}` + تدقيق `app.create`)، `PUT /apps/{app}/credentials` (`previous`/`current`/`reminder` + تدقيق `app.credentials.replace`)، `GET /apps/{app}/credentials` (metadata + `uploaded_by`/`uploaded_at` + `disabled`)، `POST /apps/{app}/disable` و`POST /apps/{app}/enable` (تدقيق)، `DELETE /apps/{app}` (body `{confirm}` يجب أن يساوي اسم التطبيق + `require_reauth(300)`). **كل معالج يبدأ بـ`user.require_owner()`** ثم `enabled` ثم المنطق ✓. |
| §3 ترتيب الحذف | تم | (1) `ConfigStore::export` يُكتب كاملاً في `audit_log` (`app.delete`) **قبل** أي حذف، وفشل التدقيق يوقف العملية؛ (2) `remove(app)`؛ (3) `ConfigStore::delete_app`؛ (4) حذف `devices` و`revoke` كل `app_keys` وحذف `template_dedupe`؛ (5) `notifications_history` و`campaigns` و`audit_log` تبقى ✓ `delete_app_keeps_history`. تطبيق غير موجود ⇒ 404. |
| §3 `GET /apps` و`GET /apps/{app}` | تم | يضيفان `disabled: bool`، و`has_credentials` يبقى حقيقة الخريطة. |
| §3 خريطة الأخطاء | تم | `Invalid`/`ProjectMismatch`/`LiveCheckFailed` ⇒ 400 برسالة عامة، `Exists` ⇒ 409 `{"error":"credentials_exist"}`، `NotFound` ⇒ 404، `Disabled` ⇒ 404، `Io` ⇒ 500 عامة. |
| §3 الحدود | تم | حجم الجسم ≤ 16KB يُفحص في المعالج ⇒ 413 `payload_too_large`؛ حد 10 عمليات/دقيقة لكل مستخدم داشبورد (`auth_fail_limiter` الموجود 10/دقيقة) بمفتاح `admin:credentials:<id>` ⇒ 429. |
| §4 `ConfigStore::delete_app` | تم | يحذف `app_configs` وكل `templates` في **معاملة واحدة** تحت قفل الكتابة، ويحدّث المخبأ، ويسجّل نسخة في `config_versions` (القسم `deleted`) وتدقيقاً `config.delete_app`؛ غير موجود ⇒ `NotFound`. اختبارات: `delete_app_removes_config_and_templates`، `delete_app_unknown_is_not_found`، `delete_app_is_atomic`. |
| §5 الاختبارات (45 اسماً) | تم | كل الأسماء المطلوبة موجودة وتعمل (فحص آلي). العدد الكلي 623 (578 قبل + 45 جديد). |
| §6 الجودة | تم | `cargo build` بلا تحذيرات، `cargo clippy --all-targets` صفر تحذير، `cargo test` = 623 ناجح/0 فاشل/0 ignored، `rustfmt --check` نظيف على الملفات المعدّلة. |
| §6 التجارب السلبية الخمس | تم | كل تجربة أخفقت الاختبار المحدد ثم أُعيد الملف بنفس `sha256` (§3). |
| §7 الـ commits | تم | (1) `feat(credentials): T16-1 validate and install service accounts atomically`، (2) `feat(credentials): T16-2 disable, enable and remove with persisted state`، (3) `feat(config): T16-3 add delete_app`، (4) `feat(admin): T16-4 add app create, credentials, disable and delete routes`، (5) `docs(plan): T16 update execution tracker`. |

## 2. الملفات

`git diff --stat 2bf54f8..HEAD -- src/ .env.example`:

```
 src/admin/apps.rs        |   57 ++-
 src/admin/credentials.rs |  849 +++++++++++++++++++++++++++++++++++++++++++++++++++++++
 src/admin/mod.rs         |  139 ++++++++
 src/config/store.rs      |  178 ++++++++++++++++++
 src/credentials.rs       | 1229 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++-
 src/db.rs                |  137 ++++++++
 src/devices.rs           |   10 +
 src/main.rs              |   68 ++-
 8 files changed, 2622 insertions(+), 45 deletions(-)
```

- **`src/credentials.rs` (تعديل كبير):** `CredentialManager` و`CredentialVerifier` و`GoogleVerifier` و`ValidatedKey` و`CredentialMeta` و`InstallOutcome` و`CredentialError` + `parse_and_validate` + `app_name_ok` + `write_atomic` + 29 اختباراً.
- **`src/admin/credentials.rs` (جديد):** المسارات الستة + `CredentialAdminError` (409/413 بأجسامها) + 18 اختباراً.
- **`src/admin/mod.rs` (تعديل):** `pub mod credentials;` + ستّة مسارات في `admin_routes()` + ستّ حالات في المصفوفة + `test_state()` يبني المدير على مجلد مؤقت + `setup_with_upload` (رفع معطّل) + `AcceptingVerifier`.
- **`src/admin/apps.rs` (تعديل):** `disabled: bool` في `GET /apps` و`GET /apps/{app}`.
- **`src/config/store.rs` (تعديل):** `delete_app` + 3 اختبارات (+178/−0، إضافة فقط ✓).
- **`src/db.rs` (تعديل):** الجدولان + 9 دوال.
- **`src/main.rs` (تعديل):** `CREDENTIALS_DIR`/`CREDENTIALS_UPLOAD_ENABLED`، بناء المدير، تحميل التطبيقات المعطّلة قبل المسح، حقل `AppState`، ومساعد الاختبارات.
- **`src/devices.rs` (تعديل):** بناء `AppState` في اختبار المساعد فقط (سطران) ✓ كما سمحت الخطوة.
- **`.env.example`:** `CREDENTIALS_DIR` و`CREDENTIALS_UPLOAD_ENABLED`.

## 3. الأوامر التي شغلتها ونتائجها

```
$ cargo build
    Finished `dev` profile [unoptimized + debuginfo] target(s)      # بلا تحذيرات

$ cargo test
   test result: ok. 623 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 53.80s

$ cargo clippy --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.03s      # صفر تحذير

$ rustfmt --check --edition 2021 --config skip_children=true \
      src/credentials.rs src/admin/credentials.rs src/admin/mod.rs src/admin/apps.rs \
      src/db.rs src/main.rs src/config/store.rs src/devices.rs
   (بلا مخرجات)                                                            # نظيف

$ cargo test -- --list | grep -c ': test$'
623
```

**التجارب السلبية الخمس (§6) — على الشيفرة النهائية، وأُعيد كل ملف بنفس البصمة:**

| # | التغيير المؤقت | الاختبار | المخرج الفعلي | `sha256` قبل/بعد |
|---|---|---|---|---|
| (أ) | المؤقت ينتهي بـ`.json` | `install_temp_file_never_ends_with_json` | FAILED — `the temporary file must not be a json file: .{}.upload-{}.json` | `ec8384755378ed4629e9177f97142a3472fe3d4b555105e1d41d25a521450d2a` (متطابق) |
| (ب) | الاستبدال لا يقارن `project_id` | `replace_requires_same_project_id` | FAILED (قُبل الاستبدال) | نفس البصمة (متطابق) |
| (ج) | الكتابة قبل `verifier.verify` | `install_live_check_failure_writes_nothing` | FAILED (الملف كُتب) | نفس البصمة (متطابق) |
| (د) | `DELETE /apps/{app}` بلا `require_reauth` | `credentials_routes_need_recent_reauth` | FAILED — 404 بدل 403 (التطبيق حُذف) | `f213939ea5a47aa538a5079d151a3a89aa8217ca73c65bd78d52a3ce891f32d9` (متطابق) |
| (هـ) | بلا صلاحيات `0600` | `install_writes_0600_and_loads_immediately` | FAILED — `the file is private` (420 بدل 384) | نفس البصمة الأولى (متطابق) |

**فحص السر (معيار القبول 2):**

```
$ grep -rn "private_key" src/admin/ | grep -v "^src/admin/credentials.rs:.*test"   # لا مسار يردّ الملف
$ grep -rn "BEGIN PRIVATE KEY" src/ | grep -v "src/credentials.rs"                # لا مكان يكتب المفتاح
```

## 4. طريقة الفحص: كل بند من "طريقة الفحص" في الخطة/الخطوة ← النتيجة الفعلية وكيف تحققت

| البند | النتيجة | كيف تحققت |
|---|---|---|
| "المالك فقط + إعادة كلمة المرور خلال 5 دقائق" | تم | `credentials_routes_owner_only` (كل المسارات الستة ⇒ 403 للمدير) و`credentials_routes_need_recent_reauth` (إنشاء/استبدال/تعطيل/حذف ⇒ 403 `reauth_required` بلا تأكيد حديث). |
| "تحقق حي قبل الكتابة" | تم | `install_live_check_failure_writes_nothing` (الفشل الحي لا يترك ملفاً) + التجربة (ج) التي تكسر الترتيب. |
| "كتابة ذرية بصلاحيات `0600`" | تم | `install_writes_0600_and_loads_immediately` (الوضع 384 = 0600) + التجربة (هـ). |
| "لا مسار يعيد محتوى الملف أو `private_key` أو يسجله أو يدققه" | تم | `get_credentials_returns_metadata_only` و`no_route_returns_file_content` (يمرّ على كل مسارات الداشبورد ويبحث عن `BEGIN PRIVATE KEY`) و`audit_has_no_private_key`. |
| "المؤقت لا ينتهي بـ`.json`" | تم | `install_temp_file_never_ends_with_json` (يفحص اسم_format في المصدر) + `watcher_ignores_dotfiles_and_invalid_names` + `no_temp_files_left_after_failure`. |
| "الإلغاء فوري (لا مخبأ)" | تم | `disable_stops_app_immediately` و`disabled_app_not_reloaded_by_scan_or_watcher`. |
| "التصدير في التدقيق قبل الحذف" | تم | `delete_app_exports_config_to_audit_before_deleting` (صف `app.delete` يحوي `"identity"` أي التصدير الكامل). |
| "لا `unwrap/expect` خارج الاختبارات" | تم | `grep` على الملفات التي لمستها: `src/credentials.rs` و`src/admin/credentials.rs` لا تحوي `expect(`/`unwrap(` خارج `#[cfg(test)]` (و`src/devices.rs` و`src/main.rs` و`src/db.rs` لم تلمس أسطر الإنتاج فيها إلا `AppState`). |
| "الاختبار الحي الحقيقي ضد Google" | **غير منفَّذ** | لا اتصال خارجي في الاختبارات (المدقق وهمي `AcceptingVerifier`)، فالفحص الحي الحقيقي (طلب access token من Google) يبقى **يدوياً** (مذكور في §1 و§7). |
| "كل الأسماء موجودة بلا ignore" | تم | فحص آلي لـ`cargo test -- --list` ✓ (45 اسماً). |

## 5. انحرافات عن الأمر (وسببها)

1. **`parse_and_validate` صار `async`:** the PEM is parsed by building the authenticator (`ServiceAccountAuthenticator::builder(..).build()`), and that builder is async; a sync function could not detect a corrupt key as the step requires (`Invalid("the private key could not be parsed")`).
2. **الملف المؤقت يُفحص بقراءة المصدر:** the temporary file is renamed away within the same call, so no runtime observer can see its name; the test therefore reads the format string of the source (`include_str!`) and asserts that it does not end with `.json` ✓ (the same technique as the T10-B startup order test).
3. **`replace_swaps_atomically_and_keeps_old_on_load_failure` يستعمل التعطيل لإجبار فشل التحميل:** the new file is always loadable (its bytes were validated), so the only way to reach the rollback is an app that is not loaded — a disabled one. The test therefore disables the app and expects the old file back.
4. **مفتاح `0o600` عبر `tokio::fs::OpenOptions::mode`:** the step mentions `std::os::unix::fs::OpenOptionsExt`; `tokio`'s own `OpenOptions` carries the same `open(2)` flag, and using it keeps the write asynchronous (the standard library's trait is not implemented for `tokio::fs::OpenOptions`).
5. **الحد 10/دقيقة يستعمل `auth_fail_limiter`:** the existing 10-per-minute limiter; its key is `admin:credentials:<id>`, so the API failure counter is untouched. Adding a field to `AppState` would touch `src/main.rs` and the frozen `src/devices.rs` (allowed here, but a shared limiter is enough and keeps the change small).
6. **الحقول `LiveCheckFailed` و`Io` بلا حمولة:** the reason of a live check is logged (`tracing::warn!`) and never answered, and the IO error carries no path; the bodies of the errors are the fixed generic messages of the step ✓ (this also keeps the dead-code lint quiet).
7. **`GET /apps` صار حلقة بدل `map`:** the `disabled` flag needs an `await` per app, so the items are built in a loop (same fields, same order) ✓.
8. **الـcommits (2) و(3) تحملان تحذيرات dead-code في بنائها:** the manager is only reachable through the `AppState` field, which arrives with the routes in the commit (4); the intermediate commits therefore report "never used" warnings. **الحالة النهائية صفر تحذير** ✓ (المطلوب في §6).
9. **commit إضافي:** `test(admin): T11-4 ...` من الخطوة السابقة لم يتغيّر؛ في T16 لم يُضَف commit إضافي (المجموعة الخمسة كما في §7) ✓.

## 6. ملاحظات خارج النطاق

- **الفحص الحي الحقيقي لم يُنفَّذ** (لا اتصال خارجي مسموح): `GoogleVerifier` جاهز و`AcceptingVerifier` وهمي في الاختبارات. التشغيل اليدوي: رفع ملف حقيقي عبر `POST /apps` مع `CREDENTIALS_UPLOAD_ENABLED=true`.
- **`CREDENTIALS_DIR` يجب أن يكون قابلاً للكتابة من الحاوية** (شروط النشر في 3.13) — لم يُعدَّل `docker-compose.yml` (ممنوع) ✓.
- **الواجهة `AddApp.vue` و`dashboard/`** ضمن T12 ✗ (ممنوع في هذه الخطوة) ✓.
- **الملف المؤقت يُنشأ في نفس المجلد**؛ من يُنشئ `credentials/` على volume للقراءة فقط تفشل الكتابة ⇒ 500 عامة ✓.
- **`scan_credentials_dir` يستعمل `create_dir_all`** كما كان ✓ (لم يتغيّر سلوك الملفات الحالية).
- (من T11/T10) متابعات T13: توحيد `trusted_sender` كطبقة واحدة عند استخراج `build_router`، وحد `test-send` والإرسال الحر على محدّدات مشتركة، واختبار مخصص لـ`dedupe.seconds`.
- (من T10-B) `PUT /apps/{app}/campaigns` هو مسار القسم و`GET /apps/{app}/campaigns` هو القائمة — تعكس الواجهة ذلك (T12).
- (من T9) مخبأ الأدوار يُبطَل عند كل حفظ قالب، الأنماط بلا `size_limit`، `validate_data_source` يقبل `http://localhost.evil.com`، `api_fallback` لا يغطي مسارَي الأجهزة.
- مستودع `taqati`: لم يُمَس (لا commit ولا push).

## 7. أسئلة للمالك/المراجع

1. **هل يكفي `GET /apps/{app}/credentials` لعرض حالة الرفع؟** (لا يوجد مسار يعيد الملف ✓).
2. **`POST /apps/{app}/enable` بلا `require_reauth`:** التفعيل يعيد التطبيق للخدمة فوراً، والتعطيل يحتاج تأكيداً ✗ (تم حسب الخطوة). موافق؟
3. **حد 10/دقيقة على `auth_fail_limiter`:** مقبول؟ (متابعة T13 مع بقية المحددات.)
4. **مفتاح `app_credentials` لا يسجّل تاريخ التثبيت ولا التبديل** (غيره يبقى `uploaded_at` الأحدث) — موافق؟
5. **`DELETE /apps/{app}` يبقي `notifications_history` و`campaigns`** (كما نصّت الخطوة) — موافق على بقائها للaudit؟
6. **الخطوة التالية: T12** (واجهة الداشبورد، ومنها `AddApp.vue` التي تستعمل مسارات T16) ✓.

## 8. حالة git النهائية

```
$ git status --short
?? docs/plans/execution/T16.md
(بعد هذا الـ commit يصبح كل شيء نظيفاً)

$ git log --oneline -7
<آخر commit>  docs(plan): T16 update execution tracker
74a07d9 feat(admin): T16-4 add app create, credentials, disable and delete routes
3b51c3c feat(config): T16-3 add delete_app
92555ee feat(credentials): T16-2 disable, enable and remove with persisted state
53e0f69 feat(credentials): T16-1 validate and install service accounts atomically
2bf54f8 docs(plan): T11 update execution tracker
```

- بلا `git push` (لم يُنفَّذ إطلاقاً). بلا `git reset --hard`/`git clean`/`git checkout -- .`/`git stash drop`. stash الموجود (`stash@{0}` بتاريخ 2026-09-23) لم يُنشأ ولم يُمَس.
- لا قيم أسرار في أي commit أو في هذا التقرير؛ الإشارة إلى الأسرار بالمسار فقط. (مفاتيح الاختبارات مولّدة لكل اختبار، ولا تُطبع في أي ردّ ولا في أي assertion.)

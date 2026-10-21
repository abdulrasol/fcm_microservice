# تقرير T10-A

- **التاريخ/الوقت:** 2026-10-21، من 18:20 إلى 23:50 (توقيت الجهاز).
- **الفرع:** `feature/policy-auth` (تحقّق قبل البدء وقبل كل commit؛ لم يتغيّر).
- **HEAD قبل العمل:** `721e329` (`docs(plan): T9 update execution tracker`).
- **HEAD بعد العمل:** آخر commit هو `docs(plan): T10A update execution tracker` — 7 commits جديدة إجمالاً، بلا push. (رقم آخر commit يتغيّر عند تعديل هذا التقرير نفسه، فالحالة النهائية تُقرأ بـ`git log --oneline -8`.)
- **ملف الخطوة المنفَّذ:** `docs/plans/execution/T10A.md` (لا يوجد `T10.md` في `docs/plans/execution/`؛ انظر §5).

## 1. ما نُفِّذ (بند بند من ملف الخطوة)

| البند | الحالة | ملاحظة |
|---|---|---|
| §0 التعديلات المسموحة | تم | جديد: `src/admin/apps.rs`، `src/admin/templates.rs`. تعديل: `src/admin/mod.rs` (جدول مسارات + `AdminError` + `authorize` + `Need` + `map_config_error`)، `src/admin/auth.rs` (**إضافة فقط**: `load_test_uid`)، `src/db.rs` (**قراءة فقط**: `template_metadata`، `read_audit`، `read_version` — بلا تغيير جداول)، `src/config/validate.rs` (قاعدة واحدة + اختباران فقط). `src/main.rs` **لم يُمَس**: `AppState` كان يحتوي كل ما يلزم (انظر §5). `Cargo.toml` و`dashboard/` و`src/config/**` وملفات أخرى لم تُمَس. |
| §1 المصادقة | تم | كل المسارات تحت `/api/v1/admin` تمر بـ`AuthedUser` (جلسة + `must_change_password` + CSRF القائمة في `csrf_guard`). لا مسار يقبل مفتاح خدمة: طبقة الأداة لا تقرأ `SERVICE_API_KEY` إطلاقاً. |
| §1 الصلاحية قبل أي عمل | تم | مساعد واحد `authorize(state, user, app, need)` في `src/admin/mod.rs:239` مع `Need ∈ {AnyMember, Owner}`. الترتيب: الدور ← ربط التطبيق ← الوجود. مدير غير مرتبط ⇒ 403 دائماً (حتى لتطبيق غير موجود)؛ owner لتطبيق غير موجود ⇒ 404. |
| §1 خريطة أخطاء `ConfigError` | تم | `map_config_error` في `src/admin/mod.rs:215`: `Invalid` ⇒ 400 `{"error":"invalid","issues":[{"path","message"}]}`، `Conflict{current_version}` ⇒ 409 `{"error":"conflict","current_version":N}`، `NotFound` ⇒ 404، `Db` ⇒ 500 `{"error":"internal","message":"the request could not be completed"}` (التفصيل في `tracing` فقط). |
| §1 حدود الجسم | تم | لم تُزد حدود الجسم؛ الحد العام 64KB كما هو (`AdminJson` في `src/admin/mod.rs`). |
| §1 عدم إرجاع الأسرار | تم | `GET /apps` يعيد `project_id` فقط من `project_map`، و`GET /apps/{app}` يعيد `config` (أسماء `secret_env` فقط، لا قيم) و`templates` (ملخصات). الاختبار `list_apps_never_exposes_credentials_content` و`export_has_no_secrets` و`audit_never_contains_secret_values` تتحقق من غياب `private_key`/`BEGIN PRIVATE KEY`/`client_email`/كلمة مرور الاختبار/توكن الجلسة. |
| §2 `GET /apps` | **جزئي** | لكل عنصر: `{app, configured, has_credentials, project_id, version, template_count, enabled_templates}`. المدير يرى تطبيقاته فقط. **الـowner لا يرى اتحاد (المُعدَّة ∪ `project_map`)** لأن `ConfigStore` لا يملك دالة تعداد و`src/config/**` ممنوع (§5 و§7). |
| §2 `GET /apps/{app}` | تم | `{app, version, config: {identity, data_source, devices, web_subscribe, campaigns, app_check}, templates: [{key, description, enabled, version, senders, target_kind, last_used_at}]}` بلا تعريف القوالب الكامل. |
| §2 `POST /apps/{app}/init` | تم | owner فقط؛ تطبيق بلا credentials ⇒ 400، تطبيق له credentials بلا إعداد ⇒ 201 (`create_app`)، موجود مسبقاً ⇒ 409 مع `current_version`. |
| §2 `PUT /apps/{app}/{section}` | تم | جسم `{value, expected_version}`؛ `identity|data_source|devices|app_check` للمالك فقط، `web_subscribe|campaigns` للمالك والمدير لتطبيقه؛ قسم مجهول ⇒ 404؛ النتيجة `{version}`؛ كل الحواجز تمر عبر `ConfigStore::save_section`. |
| §2 قيد المدير على `campaigns` | تم | المدير لا يغيّر `campaigns.senders` ولا `campaigns.entity`: يُقارَن القسم المقبول بالقسم الحالي ويُرفض بـ403 إن تغيّر أحدهما. المالك بلا قيود. الاختباران: `manager_cannot_change_campaign_senders_or_entity` و`manager_can_change_web_subscribe`. |
| §3 قوالب: قائمة/تفاصيل | تم | `GET /apps/{app}/templates` و`GET /apps/{app}/templates/{key}` (تعريف كامل + `version` + `enabled` + `last_used_at`). |
| §3 قوالب: إنشاء/تعديل/تفعيل/حذف | تم | `POST` (مكرر ⇒ 409)، `PUT` (بلا `expected_version` ⇒ 400؛ قديم ⇒ 409)، `PUT .../enabled` (204)، `DELETE` (409 `recently_used` إن استُخدم خلال 30 يوماً، وإلا 204). اسم المفتاح `[a-z0-9_]{1,40}` ⇒ 400. |
| §3 `POST .../templates/validate` | تم | `200 {"ok":bool,"issues":[...]}` على الإعداد الحالي بلا حفظ؛ أخطاء الصياغة والحقول المجهولة تُرجَع كـissues لا 500. |
| §3 `POST .../templates/{key}/preview` | تم | يستعمل القالب المحفوظ أو `definition` في الجسم (ويجب أن يمر `validate_template` وإلا 400). `doc` يدوية تحل محل قراءة المصدر، و`use_real_doc:true` يقرأ عبر `state.datasource` (503 عند تعطل المصدر). ينفّذ `validate_params` ثم `render` ويعيد `{title, body, image, data, target:{kind,value}, dedupe_key, recipients:{kind,count}}`. `require` الفاشل ⇒ 200 `{"rendered":false,"reason":"forbidden"}` بلا كشف بيانات الوثيقة. **لا يستدعي FCM إطلاقاً**. |
| §3 `POST .../templates/{key}/test-send` | تم | يرسل إلى أجهزة `tool_users.test_uid` للمستخدم الحالي فقط (يتجاهل `to`)، بلا `test_uid` ⇒ 400 `Set a test_uid on your account first`، بلا حد قالب ولا dedupe ولا senders، بحد 10/دقيقة لكل مستخدم. السجل `kind="test"`, `ref=<key>`. لا أجهزة ⇒ 200 `{"delivered":0,"reason":"no_devices"}`. `test_uid` يُقرأ من قاعدة البيانات لا من الجسم (الجسم يرفضه `deny_unknown_fields`). |
| §4 الإصدارات | تم | `GET .../versions?section=&limit=` بلا `content` (فقط `id, section, changed_by, created_at, size`)، و`GET .../versions/{id}` يعيد المحتوى لمن له صلاحية قسمه. |
| §4 `restore` | تم | owner لكل الأقسام؛ manager لـ`web_subscribe`/`campaigns`/anything starting with `template:` فقط؛ نسخة تخص تطبيقاً آخر ⇒ 404؛ الاستدعاء عبر `ConfigStore::restore`. |
| §4 `export` / `import` | تم | `GET .../export` (owner) = `ConfigStore::export`. `POST .../import` (owner) يتطلّب `require_reauth(300)` وإلا 403 `reauth_required`، ثم `ConfigStore::import`. |
| §4 `GET /audit` | تم | owner ⇒ كل التطبيقات (أو مفلتر)، manager ⇒ `app` إلزامي وداخل تطبيقاته وإلا 403. `limit ≤ 200`، ترتيب تنازلي بالمعرّف، `before` للتصفح. يعيد `{id, actor, app, action, created_at}` و`detail` مقتطعاً إلى 500 حرف. |
| §5 الاختبارات (56 اسماً) | تم | كل الأسماء الستة والخمسين موجودة وتعمل؛ انظر §3 و§4. توزيعها: `src/admin/mod.rs` 7 (مصفوفة) + `src/admin/apps.rs` 22 + `src/admin/templates.rs` 25 + `src/config/validate.rs` 2. |
| §6 (أ) `require` لا يقبل `{var}` ولا `{doc.*}` | تم (+ قاعدة) | `context.require` كان متساهلاً ⇒ أُضيفت قاعدة واحدة في `src/config/validate.rs:543-566` ("require values may only use '{p.*}' and '{sender.*}'") مع اختبارَي `require_rejects_var_reference` و`require_rejects_doc_reference`. `campaigns.entity.require` كان محكوماً أصلاً بـ`check_campaign_placeholders` (`src/config/validate.rs:952-960`، يسمح بـ`p.*`/`sender.*` فقط). `campaigns.content.actions.*.check.require` **كان مُتساهلاً أيضاً** (لا يفحص إلا وجود `{sender.uid}`) ⇒ طبّقت القاعدة نفسها على هذا الموضع بالمساعد الموجود `check_campaign_placeholders` (رمز `[C5]`)، والاختباران يغطّيان الموقعين. |
| §6 (ب) `dedupe.seconds` ضمن 1..=86400 | محكوم مسبقاً | القاعدة موجودة في `src/config/validate.rs:639-644` ("[T7] dedupe.seconds must be between 1 and 86400")، لكن **لا يوجد اختبار مخصص** لها. لم يُضَف اختبار (§0 يسمح بقاعدة واحدة واختبارها فقط، والاستثناء كان في §6(أ)). |
| §7 الجودة | تم | `cargo build` بلا تحذيرات، `cargo clippy --all-targets` صفر تحذير، `cargo test` = 450 ناجح/0 فاشل/0 ignored، `rustfmt --check` نظيف على الملفات المعدّلة. |
| §7 التجارب السلبية الخمس | تم | كل تجربة أخفقت الاختبار المحدد ثم أُعيد الملف بنفس `sha256` (§3). |
| §8 الـ commits | تم (+ commit إضافي) | 5 commits كما في الخطوة (4 كود + 1 توثيق) + commit مستقل للفقرة 6 + commit إضافي واحد (انظر §5). |

## 2. الملفات

`git diff --stat 721e329..HEAD -- src/`:

```
 src/admin/apps.rs      |  913 +++++++++++++++++++++++++++++++++++
 src/admin/auth.rs      |   16 +-
 src/admin/mod.rs       |  946 ++++++++++++++++++++++++++++++++++++-
 src/admin/templates.rs | 1233 ++++++++++++++++++++++++++++++++++++++++
 src/config/validate.rs |  114 +++++
 src/db.rs              |  143 ++++++
 6 files changed, 3336 insertions(+), 29 deletions(-)
```

- **`src/admin/apps.rs` (جديد):** تطبيقات وأقسام وإصدارات واستيراد/تصدير/تدقيق: `list_apps`، `get_app`، `init_app`، `put_section`، `list_versions`، `get_version`، `restore_version`، `export`، `import`، `audit` + 22 اختباراً.
- **`src/admin/templates.rs` (جديد):** قوالب: `list_templates`، `get_template`، `create_template`، `update_template`، `set_enabled`، `delete_template`، `validate_definition`، `preview`، `test_send` + مساعدات `prepare`/`recipient_count` + 25 اختباراً.
- **`src/admin/mod.rs` (تعديل):** جدول المسارات `admin_routes()` + `merge_routes()`، `AdminError` (مع `body` اختياري للأجسام الموثّقة `invalid`/`conflict`/`recently_used`)، `map_config_error`، `Need`، `authorize`، وحدة اختبارات مشتركة (حزمة + `Harness` + `CountingFcm` + `FakeEnv`) + 7 اختبارات مصفوفة.
- **`src/admin/auth.rs` (تعديل، إضافة فقط):** `load_test_uid` (قراءة `tool_users.test_uid` للمستخدم من الجلسة).
- **`src/db.rs` (تعديل، قراءة فقط):** `TemplateMeta` + `template_metadata` (المفتاح/الإصدار/`last_used_at`/علامة `recently_used` بـ`datetime('now','-30 days')`)، `AuditRow` + `read_audit` (بترتيب تنازلي بالمعرّف + `before` + `limit`)، `VersionDetail` + `read_version` (مقيّد بالتطبيق). **بلا INSERT/UPDATE/DELETE وبلا تغيير جداول.**
- **`src/config/validate.rs` (تعديل، استثناء §6):** قاعدة `context.require` (سطر 543-566) + توسيعها إلى `check.require` (سطر 1067-1093) + اختبارَين.
- **مستندات:** `docs/plans/execution/T10A.md` (مضاف إلى git)، `docs/plans/execution/README.md` (تحديث المتتبّع)، `docs/plans/secure-sending-plan.md` (T9=✅، T10=🔄، وسجل التقدم).

## 3. الأوامر التي شغلتها ونتائجها

```
$ cargo build
   Compiling fcm_microservice v0.1.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 10.79s      # بلا تحذيرات

$ cargo test
   test result: ok. 450 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 28.67s

$ cargo clippy --all-targets
    Checking fcm_microservice v0.1.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.90s      # صفر تحذير

$ rustfmt --check --edition 2021 --config skip_children=true \
      src/admin/mod.rs src/admin/apps.rs src/admin/templates.rs \
      src/admin/auth.rs src/db.rs src/config/validate.rs
   (بلا مخرجات)                                                            # نظيف

$ cargo test -- --list | grep -c ': test$'
450                                                                        # 394 قبل + 56 جديد
```

**اختبارات هذا العمل (56):**

```
$ cargo test admin::tests::matrix
running 7 tests
test admin::tests::matrix_manager_allowed_on_own_app_routes ... ok
test admin::tests::matrix_manager_denied_owner_only_routes ... ok
test admin::tests::matrix_manager_other_app_is_403_not_404 ... ok
test admin::tests::matrix_missing_csrf_header_is_403_on_writes ... ok
test admin::tests::matrix_must_change_password_blocks_new_routes ... ok
test admin::tests::matrix_owner_can_reach_every_route ... ok
test admin::tests::matrix_unauthenticated_is_401_on_every_route ... ok
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 443 filtered out

$ cargo test admin::apps::tests      → 22 passed; 0 failed
$ cargo test admin::templates::tests → 25 passed; 0 failed
$ cargo test config::validate::tests::require_rejects
test config::validate::tests::require_rejects_doc_reference ... ok
test config::validate::tests::require_rejects_var_reference ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 448 filtered out
```

**التجارب السلبية الخمس (§7) — نُفِّذت على الشيفرة النهائية، وأُعيد كل ملف بنفس البصمة:**

| # | التغيير المؤقت | الاختبار | المخرج الفعلي | `sha256` قبل/بعد |
|---|---|---|---|---|
| (أ) | إسقاط `user.require_owner()` لأقسام `identity\|data_source\|devices\|app_check` في `put_section` | `matrix_manager_denied_owner_only_routes` | FAILED — `PUT /apps/watt/identity was not refused` (400 بدل 403) | `746e823b466fc87826273b4500f942a0432ef76deb6a1877003c8efa4a00c16d` (متطابق) |
| (ب) | جعل التطبيق غير موجود ⇒ 404 قبل `require_app` في `authorize` | `matrix_manager_other_app_is_403_not_404` | FAILED — `app missing: {"error":"not_found"}` (404 بدل 403) | `a553886139733ba24eae9d4cf8a2863e47c8614e4f812f37608087ebe80f9fa0` (متطابق) |
| (ج) | قراءة `test_uid` من جسم `test-send` (حقل `test_uid` اختياري) | `test_send_body_cannot_override_test_uid` | FAILED — 200 بدل 400 (أُرسل إلى جهاز `other`) | `2cf9ee3db2af22fd2f4fb2c638d158f3be8491d629b7dc777cec5a56a92a1c26` (متطابق) |
| (د) | استدعاء `state.fcm.send(...)` داخل `preview` | `preview_never_calls_fcm` | FAILED — `preview must never reach FCM: left: 1, right: 0` | نفس البصمة أعلاه (متطابق) |
| (هـ) | حذف فحص الـ30 يوماً في `delete_template` | `template_delete_recently_used_is_409` | FAILED — 204 بدل 409 | نفس البصمة أعلاه (متطابق) |

**تجربة إضافية (تأكيد معيار القبول 3):** أضفت مساراً وهمياً `("GET", "/audit/extra", ...)` إلى جدول المسارات ⇒ فشل `matrix_owner_can_reach_every_route` بالرسالة:
`the owner matrix must cover every registered route` (يسار: 25 مساراً مغطى / يمين: 26 مسجّل) ⇒ أُعيد الملف بنفس البصمة `a553886139733ba24eae9d4cf8a2863e47c8614e4f812f37608087ebe80f9fa0`.

## 4. طريقة الفحص: كل بند من "طريقة الفحص" في الخطة/الخطوة ← النتيجة الفعلية وكيف تحققت

| البند (خطة T10 / خطوة T10-A) | النتيجة | كيف تحققت |
|---|---|---|
| "اختبارات صلاحيات لكل مسار (owner/manager/غير مرتبط)" | تم | 7 اختبارات مصفوفة. المسارات مستخرجة من **جدول المسارات نفسه** (`admin_routes()`) لا من قائمة مكتوبة يدوياً: `matrix_unauthenticated_is_401_on_every_route` يمر على كل مسار محمي (22 مساراً) ويتوقع 401؛ `matrix_owner_can_reach_every_route` يغطي **كل** المسارات المسجّلة (25) ويتأكد أنها ليست 401/403؛ و`matrix_manager_denied_owner_only_routes` (7 مسارات owner-only ⇒ 403)، `matrix_manager_allowed_on_own_app_routes` (13 مساراً لتطبيقه ⇒ ليس 403)، `matrix_manager_other_app_is_403_not_404` (تطبيق غير مرتبط وغير موجود ⇒ 403 لا 404)، `matrix_must_change_password_blocks_new_routes` (403 `password_change_required`)، `matrix_missing_csrf_header_is_403_on_writes` (403). |
| "`preview` لا يستدعي FCM" | تم | `preview_never_calls_fcm` (عدّاد `CountingFcm` = 0) + تجربة (د) التي أفشلت الاختبار عند إضافة `fcm.send`. |
| "`test-send` بلا `test_uid` ⇐ 400" | تم | `test_send_without_test_uid_is_400` (400 + الرسالة `Set a test_uid on your account first`). |
| "حذف قالب مستخدم حديثاً ⇐ 409" | تم | `template_delete_recently_used_is_409` (409 `recently_used` + `hint`) و`template_delete_old_usage_ok` (40 يوماً ⇒ 204) + تجربة (هـ). |
| "كل تعديل يظهر في `audit_log`" | تم | `audit_shows_every_write_above` يتحقق من وجود `config.save_section.web_subscribe` و`config.save_template` و`config.import` في سجل التطبيق. |
| "لا SQL على جداول الإعداد خارج `ConfigStore`" | **جزئي** | لا SQL على `app_configs` إطلاقاً خارج المخزن. **لكن** `db.rs` يقرأ `templates` (`template_metadata`) و`config_versions` (`read_version`) و`audit_log` (`read_audit`) لأن §3/§4 تشترط قراءة `last_used_at` وقراءة قسم النسخة قبل الاستعادة وقراءة التدقيق، و`ConfigStore` لا يوفّر ذلك. انظر §5. |
| "لا استدعاء FCM حقيقي / لا شبكة حقيقية في الاختبارات" | تم | `CountingFcm` يحل محل `HttpFcm` في كل اختبارات هذا العمل؛ مصدر البيانات في اختبار واحد فقط هو `TestServer` المحلي (`src/datasource/test_support.rs`). |
| "لا `unwrap()`/`expect()` خارج الاختبارات" | تم | `cargo clippy --all-targets` صفر تحذير (و`unwrap_used`/`expect_used` غير مفعّلين افتراضياً، لكن المراجعة اليدوية للكود الجديد: كل `expect` داخل `#[cfg(test)]` أو في بناء الحالة الاختبارية). |
| "لا `std::env` في الكود الجديد" | تم | `FakeEnv` يعيد قيمة ثابتة لاسم واحد فقط؛ لا `std::env` في `src/admin/apps.rs` ولا `src/admin/templates.rs` (المتحقق بـ grep). |
| "لغة `src/` إنجليزية" | تم | كل التعليقات والرسائل إنجليزية. **لا توجد بيانات اختبار عربية** في هذا العمل (0 أسطر عربية في الملفات المعدّلة). |

## 5. انحرافات عن الأمر (وسببها)

1. **اسم ملف الخطوة:** لا يوجد `docs/plans/execution/T10.md`؛ الملف الوحيد الموجود هو `T10A.md` (أُنشئ 18:23 وهو غير متتبَّع في git قبل هذا العمل). نُفِّذ `T10A.md` حرفياً ووُثِّقت الشريحة A فقط.
2. **`GET /apps` للـowner غير مكتمل (الاتحاد):** الخطوة تطلب "اتحاد (المُعدَّة في المخزن ∪ مفاتيح `project_map`)" لكن `ConfigStore` لا يملك دالة تعداد للتطبيقات (المتاح: `get`، `app_count`، `create_app`، `save_section`، `save_template`، `set_template_enabled`، `delete_template`، `restore`، `export`، `import`، `versions`)، و§0 يمنع تعديل `src/config/**` ("إن احتجت دالة جديدة فاكتب الحاجة في التقرير ولا تنفذها"). لذلك يعرض الـowner **مفاتيح `project_map` فقط**، فالتطبيق المُعدّ بلا credentials لا يظهر. الحل المطلوب: `ConfigStore::apps() -> Vec<String>` (قراءة فقط) — يُنفَّذ في شريحة لاحقة إن سمح المراجع بتعديل `src/config/store.rs`.
3. **`POST /apps/{app}/init` يستعمل `require_owner()` بدل `authorize()`:** مع `authorize` كان التطبيق غير المعروف (بلا credentials ولا إعداد) يُرجع 404، فتصبح قاعدة "تطبيق بلا credentials ⇒ 400" غير قابلة للوصول. لذلك: فحص الدور أولاً (403 للمدير)، ثم credentials (400)، ثم وجود الإعداد (409). التطبيق على `project_map` فقط.
4. **`GET /apps/{app}` يرجّع 404 لتطبيق له credentials بلا إعداد:** لا يوجد إعداد لعرضه، و`ConfigStore::get` يعيد `None`. الرسالة `not_found` ("the app is not configured"). قرار مُوثَّق (§7).
5. **حد `test-send` يعيد استخدام `AppState::auth_fail_limiter`:** §3 يقول "لا تضف حقلاً جديداً"؛ و`src/devices.rs` (ممنوع) يبني `AppState` في اختباراته، فإضافة حقل تكسره. استُعمل نفس `RateLimiter` (10/دقيقة) بمفتاح مستقل `admin:test_send:<user_id>` ⇒ لا يتعارض مع حد فشل المصادقة (مفتاحه مختلف) ولا يحتاج تعديل `src/main.rs`.
6. **قراءة SQL خارج `ConfigStore` في `db.rs`:** `template_metadata` و`read_version` و`read_audit` (قراءة فقط). `app_configs` لا يُقرأ خارج المخزن. السبب: §3 يطلب قراءة `last_used_at` بدالة في `db.rs`، و§4 يطلب "يُقرأ قسم النسخة من الجدول قبل الاستعادة" و"دالة القراءة في `db.rs`" للتدقيق. `ConfigStore` لا يوفّر هذه القراءات.
7. **قاعدة §6 طُبّقت على موقعين:** `context.require` (كان مُتساهلاً) و`campaigns.content.actions.*.check.require` (كان مُتساهلاً أيضاً: لم يفحص إلا وجود `{sender.uid}`). القاعدة **واحدة** ("require values may only use '{p.*}' and '{sender.*}'") مطبَّقة في الموقعين؛ الاختباران المطلوبان يغطّيان الموقعين. `campaigns.entity.require` كان محكوماً أصلاً بـ`check_campaign_placeholders`.
8. **commit إضافي سادس:** `255f642 refactor(admin): T10A-4 build the router from one route table`. أُضيف لأن معيار القبول 3 يطلب أن "يعدّ اختبار المصفوفة كل المسارات المسجلة (يفشل إن أُضيف مسار بلا تغطية)"؛ استُخرج جدول المسارات في `admin_routes()` وبُني منه الموجّه، فيفشل الاختبار تلقائياً عند إضافة مسار. Step §8 كان يعدّ 5 commits + commit الفقرة 6؛ صار 7 (6 كود/توثيق + 1 مستقل).
9. **`role` بلا `identity.role_members` ⇒ `count: null`:** الخطوة تقول "role ⇒ عدد أعضاء الدور (من `role_directory`)"، لكن `role_members` اختياري في الإعداد (افتراضياً `None`)، فعدد الأعضاء لا يمكن جلبه ⇒ رُدّ `null` بدل 400 (سلوك fail-closed: لا يُخمَّن عدد). `topic/condition ⇒ null` حسب الخطوة.
10. **لا اختبار مخصص لـ`dedupe.seconds`:** القاعدة موجودة (§6(ب)) لكن §0 يسمح بقاعدة واحدة واختبارها فقط، والاستثناء كان في §6(أ). مُوثَّق في §1 و§7.
11. **`src/main.rs` لم يُمَس:** لم يكن ضرورياً (كل الحقول موجودة في `AppState`).

## 6. ملاحظات خارج النطاق

- (من T9) مخبأ الأدوار يُبطَل عند كل حفظ قالب، فتصبح تعديلات القوالب مكلفة على تطبيقات كبيرة.
- (من T9) أنماط الأدوار تُترجم بلا `size_limit` ولا تثبيت (anchoring) فيبقي سلوك `validation.rs` خارج هذا الملف.
- (من T9) `validate_data_source` يقبل `http://localhost.evil.com` (تحقّق `ends_with` على النطاق).
- (من T9) `api_fallback` لا يغطي مسارَي الأجهزة (`/devices/register`، `/devices/unregister`).
- (من T9) أسئلة مفتوحة: حقن العنوان 201 بدل 400 (مقبول بقرار المراجع)، تجاوز `check.require` الذي يحوي `{sender.` في حالة مفتاح الخدمة، هل يفرض `GET`/`cancel` قيود `senders`، بقاء `#[allow(dead_code)]` على `claim_for_sending` حتى T10-B، الحملات العالقة في `sending` بعد انهيار العملية (نطاق T10-B).
- (من T2) تغطية ES256 غير مختبَرة، ورفض `alg=none` غير مغطّى، وشكل `tracing::error!(secret_env = ...)`، وهل يمدّد `reauth` عمر `expires_at`.
- القيد على المدير في `campaigns` يغطي `senders` و`entity` فقط؛ يستطيع المدير تغيير `review.mode` و`quota` و`dedupe` و`content` (وهو ما تطلبته الخطوة حرفياً).
- مستودع `taqati`: لم يُمَس (لا commit ولا push). تدوير `API_KEY` القديم (R5) ما زال مفتوحاً.

## 7. أسئلة للمالك/المراجع

1. **هل يُسمح بإضافة `ConfigStore::apps()` (قراءة فقط) في `src/config/store.rs`؟** بدونها لا يمكن تنفيذ "اتحاد التطبيقات" في `GET /apps` للـowner، ويبقى التطبيق المُعدّ بلا credentials غير مرئي.
2. **`GET /apps/{app}` لتطبيق له credentials بلا إعداد ⇒ 404 `not_found` ("the app is not configured")** — مقبول؟ أم تُفضَّل رسالة/رمز مختلفان يميّز "غير مُعدّ" عن "غير موجود"؟ (اختير 404-uniform لتفادي كشف وجود التطبيق قبل تسجيله).
3. **`POST /apps/{app}/init` بلا `authorize`** (دور فقط ثم credentials) — موافق؟ البديل: `authorize` أولاً (يعيد 404 للتطبيق المجهول) ويصير 400 غير قابل للوصول.
4. **هل `count: null` لهدف `role` بلا `role_members` مقبول؟** أم يجب 400 كما كان السلوك السابق في مسار القالب `/api/v1/send/template`؟
5. **هل تُضاف دالة `ConfigStore` لقراءة `templates` و`config_versions` بدل قراءات `db.rs`؟** (الوضع الحالي يكسر المعيار 1 حرفياً لكنه مطلوب في §3/§4.)
6. **هل يُضاف اختبار مخصص لـ`dedupe.seconds` (1..=86400)؟** القاعدة موجودة في `src/config/validate.rs:639-644` بلا اختبار.
7. **حد `test-send` معاد استخدامه من `auth_fail_limiter`:** مقبول؟ (المفتاح منفصل: `admin:test_send:<user_id>`.)
8. **التالي:** الشريحة B (الحملات: موافقة/رفض/حظر، السجل، الإرسال الحر، إحصاءات الأجهزة، تعافي الحملات العالقة) في `src/admin/campaigns.rs`، ثم الشريحة C (حسابات الأداة + `identity/test` + `data_source/test`).

## 8. حالة git النهائية

```
$ git status --short
 M docs/plans/execution/README.md
 M docs/plans/secure-sending-plan.md
?? docs/plans/execution/T10A.md
(بعد هذا الـ commit يصبح كل شيء نظيفاً)

$ git log --oneline -7
<آخر commit>  docs(plan): T10A update execution tracker
255f642 refactor(admin): T10A-4 build the router from one route table
5273fba fix(config): T10A-6 restrict require values to sender and params
e66d4c9 feat(admin): T10A-4 add versions, import/export and audit routes
abf8cbc feat(admin): T10A-3 add template preview and test-send
6c8d5ed feat(admin): T10A-2 add template CRUD, validate and enable routes
fcec3f3 feat(admin): T10A-1 add route authorization helper and app routes
721e329 docs(plan): T9 update execution tracker
```

- بلا `git push` (لم يُنفَّذ إطلاقاً). بلا `git reset --hard`/`git clean`/`git checkout -- .`/`git stash drop`. stash الموجود (`stash@{0}` بتاريخ 2026-09-23) لم يُنشأ ولم يُمَس.
- لا قيم أسرار في أي commit أو في هذا التقرير؛ الإشارة إلى الأسرار بالمسار فقط.

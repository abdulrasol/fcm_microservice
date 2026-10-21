# تقرير T6
- التاريخ/الوقت: 2026-10-21 14:29 +03
- الفرع و HEAD قبل/بعد: `feature/policy-auth`، قبل `257e38e`، بعد `51126d3` (ثم commit التوثيق هذا).

## 1. ما نُفِّذ (بند بند من ملف الخطوة)

| البند | الحالة | ملاحظة |
|---|---|---|
| §0 التعديلات المسموحة | تم | جديد `src/devices.rs`، وتعديل `src/db.rs` و`src/main.rs`، و`src/auth.rs` بإضافة `authenticate_identity` فقط. استثناء إلزامي واحد: `src/admin/mod.rs` داخل `test_state()` (سطران + `pool.clone()`) لأن إضافة حقلي `AppState` كسرت بناء الاختبارات (انظر §5). |
| §1 جدول `devices` | تم | في `migrate`: الأعمدة الستة حرفياً كما في 3.14، `PRIMARY KEY(app, token)`، و`CREATE INDEX IF NOT EXISTS idx_devices_uid ON devices(app, uid)`. `migration_is_idempotent` تشغّل `migrate` ثلاث مرات وتبقى الصفوف سليمة. |
| §2 `Platform` | تم | `serde(rename_all = "lowercase")`: `android`/`ios`/`web` فقط؛ أي قيمة أخرى تفشل في serde ⇒ 400 في المسار. |
| §2 `validate_token` | تم | 32..=4096 بايت، والمحارف `[A-Za-z0-9:_\-.]` فقط؛ رسالة الخطأ لا تحتوي التوكن. |
| §2 `DeviceStore::new/register` | تم | `register` يتحقق من التوكن، يقصّ `max_per_uid` ضمن 1..=20، ويجري upsert + عدّ + إخلاء داخل معاملة واحدة تحت `write_lock`. `ON CONFLICT(app, token)` ينقل الملكية ويحدّث `last_seen` ولا يغيّر `created_at`. |
| §2 الإخلاء | تم | `DELETE ... WHERE rowid IN (SELECT ... ORDER BY last_seen ASC, rowid ASC LIMIT ?)` مع استثناء `token <> ?` للجهاز الجديد؛ يعيد عدد المحذوفات. |
| §2 `unregister` | تم | `DELETE ... WHERE app = ? AND token = ? AND uid = ?`؛ يعيد `bool`. لا يميّز الملكية عن عدم الوجود في الرد. |
| §2 `tokens_for_uid` | تم | استعلام bind واحد مرتب `last_seen DESC, rowid ASC`. |
| §2 `tokens_for_uids` | تم | `QueryBuilder` مع `push_bind`، دفعات من 50 uid، إزالة التكرار عبر `HashSet`، وسقف `cap` مع توقف مبكر. |
| §2 `remove_dead_token` | تم | حذف بأي مالك حسب `(app, token)`. |
| §2 `RoleDirectory::uids_for_role` | تم | تحقق `^[A-Za-z0-9_\-]{1,64}$` قبل أي استدعاء؛ `source.members(collection, field, value, max)`؛ أي `DataError` ⇒ `DeviceError::Unavailable`؛ المخبأ بمفتاح `(app, version, role_value)` وTTL = `cache_seconds` (0 ⇒ بلا تخزين)، حد 256 (تنظيف المنتهي ثم مسح الكل)، لا يُخزَّن الخطأ، ولا يُمسك القفل عبر `await`. |
| §2 `tokens_for_role` | تم | `Option<&RoleMembers>`؛ `None` ⇒ `NotConfigured`؛ وإلا `uids_for_role` ثم `tokens_for_uids` بسقف 5000. |
| §3 المسارات | تم | `POST /api/v1/devices/register` بجسم `{app, token, platform}` و`POST /api/v1/devices/unregister` بجسم `{app, token}`، وكلاهما `deny_unknown_fields`. الردود: `200 {"registered":true,"evicted":N}` و`200 {"removed":bool}`. |
| §3 المصادقة | تم | Bearer هوية مستخدم عبر `AuthService::authenticate_identity` (دالة جديدة تعيد `Identity` كاملة). لا مفتاح خدمة ولا مفتاح قديم؛ غياب التوكن/فساده ⇒ 401، مصدر المفاتيح غير متاح ⇒ 503، تطبيق مجهول ⇒ 404 بنفس رسالة `topics.rs`. أخطاء 401/403 تمر بـ`auth_fail_limiter` عبر `auth_error_with_limit`. |
| §3 حدود السرعة | تم | `topics::check_ip_limit` + `devices_limiter: RateLimiter` جديد في `AppState` بحد 20/دقيقة لكل `uid:<app>:<uid>`؛ التجاوز ⇒ `topics::too_many_requests()` (429). |
| §3 `max_per_uid` | تم | من `config.get(app).config.devices.max_per_uid`، وتطبيق بلا إعداد ⇒ `config::Devices::default().max_per_uid` = 5. |
| §3 التسجيل | تم | `tracing::debug!(event = "devices.register", app, evicted, ...)` فقط؛ لا توكن في أي سجل. |
| §4 اختبارات store | تم | 15/15 بالأسماء المطلوبة. |
| §4 اختبارات directory | تم | 7/7 بالأسماء المطلوبة. |
| §4 اختبارات HTTP | تم | 8/8 بأسماء `oneshot` على نمط `admin/mod.rs`، على SQLite في الذاكرة. |
| §4 بلا تبعيات جديدة | تم | `Cargo.toml` لم يُعدَّل. |
| §5 الجودة والتجربتان | تم | التفاصيل في §3 و§4 أدناه، مع بيان صريح عن نتيجة التجربة (ب). |
| §6 التوثيق والـ commits | تم | 4 commits بلا push: ثلاثة كود + commit التوثيق هذا. |
| معيار القبول 1 | تم | `git diff 257e38e..HEAD --stat` محصور في القسم 0 + سطرَي `test_state()` الإلزاميين؛ `src/auth.rs` إضافة فقط (+13 سطراً، بلا حذف). |
| معيار القبول 2 | تم | لا `unwrap()`/`expect(` قبل `#[cfg(test)]` في `devices.rs`، ولا `std::env::var`، وفحص العربية صفر. |
| معيار القبول 3 | تم | `grep -n "token" src/devices.rs \| grep -i "tracing\|format!\|log"` لا يُظهر تسجيل توكن كامل: النتائج تعليق الملف، ومساعدات اختبار، وبناء ترويسة Bearer داخل الاختبارات فقط. |
| معيار القبول 4 | تم | كل الأسماء الثلاثين موجودة بلا `#[ignore]`؛ التجربتان موثقتان في §3. |
| معيار القبول 5 | تم | كل SQL إما `sqlx::query*` مع `.bind` أو `QueryBuilder` مع `push_bind`؛ لا بناء نصي بقيم المستخدم. |

## 2. الملفات

`git diff --stat 257e38e..HEAD`:
```
 src/admin/mod.rs |    4 +-
 src/auth.rs      |   13 +
 src/db.rs        |   18 +
 src/devices.rs   | 1407 ++++++++++++++++++++++++++++++++++++++++++++++++++++++
 src/main.rs      |  179 ++++++-
 5 files changed, 1619 insertions(+), 2 deletions(-)
```

- `src/devices.rs` (جديد، 1407 أسطر): `Platform` + `DeviceError` (بلا تسريب توكن) + `validate_token` + `DeviceStore` (تسجيل/إلغاء/قراءة/إزالة توكن ميت/`tokens_for_role`) + `RoleDirectory` والمخبأ + 30 اختباراً.
- `src/db.rs` (+18): جدول `devices` وفهرسه داخل `migrate` فقط.
- `src/main.rs` (+179/−2): `mod devices;` + حقلا `AppState` (`devices`، `devices_limiter`) + بناء المتجر + المساران + المعالجات + `device_error`/`ensure_known_app`/`authorize_device` + استيراد `JsonRejection`.
- `src/auth.rs` (+13): `authenticate_identity` فقط (تفويض إلى `verify_identity` الموجود).
- `src/admin/mod.rs` (+2/−1): داخل `test_state()` فقط: `db: pool.clone()` وحقلا `AppState` الجديدان. لا مسار أو منطق إداري تغيّر (انظر §5).
- `docs/plans/execution/T6.md` (كان غير متتبعاً فأُضيف)، `docs/plans/execution/README.md`، `docs/plans/secure-sending-plan.md` (§9)، `docs/plans/reports/T6-report.md`.

## 3. الأوامر التي شغلتها ونتائجها

- `cargo build` ⇒ `Finished dev profile` بلا تحذير.
- `cargo clippy --all-targets` ⇒ `Finished` بلا أي تحذير.
- `cargo test` ⇒ `test result: ok. 242 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out` (الأساس 212، والجديد 30).
- `cargo test devices -- --list` ⇒ 30 اسماً: 15 store + 7 directory + 8 HTTP، وكلها ظهرت في التشغيل الفعلي:
  `concurrent_registers_respect_limit`, `max_per_uid_is_clamped`, `migration_is_idempotent`, `platform_rejects_unknown_value`, `register_enforces_max_per_uid_evicting_oldest`, `register_inserts_and_updates_last_seen`, `register_never_evicts_the_new_device`, `register_rate_limit_per_uid_is_429`, `register_response_never_echoes_token`, `register_same_token_transfers_ownership`, `register_unknown_app_is_404`, `register_unknown_field_is_400`, `register_with_valid_jwt_identity_succeeds`, `register_without_token_is_401`, `remove_dead_token_removes_any_owner`, `role_cache_is_bounded`, `role_uids_cache_invalidated_by_config_version`, `role_uids_cached_within_ttl`, `role_uids_source_error_is_unavailable_and_not_cached`, `role_uids_zero_ttl_not_cached`, `role_value_validation_rejects_injection_like_input`, `service_key_cannot_register_devices`, `token_never_appears_in_error_text`, `token_validation_rejects_bad_charset_and_length`, `tokens_for_role_requires_role_members_config`, `tokens_for_uid_returns_only_that_app_and_uid`, `tokens_for_uids_dedups_chunks_and_caps`, `unregister_foreign_or_missing_returns_false`, `unregister_foreign_token_reports_not_removed`, `unregister_removes_only_own_token`.
- `rustfmt --edition 2021 --config skip_children=true --check src/main.rs src/devices.rs src/db.rs src/auth.rs src/admin/mod.rs` ⇒ لا فرق (exit 0) بعد التنسيق.
- فحص العربية: `perl -CSD -ne 'print ... if /[\x{0600}-\x{06FF}]/'` على الملفات المعدَّلة ⇒ صفر سطر.
- `awk '/#\[cfg\(test\)\]/{exit} ...' src/devices.rs | grep -E "\.unwrap\(\)|\.expect\(|std::env::var"` ⇒ لا شيء.
- `grep -n "token" src/devices.rs | grep -i "tracing\|format!\|log"` ⇒ التعليق العلوي، و`format!("device-token-...")` في مساعد اختبار، وسطران داخل اختبارات HTTP (`format!` لتوكن اختبار وترويسة Bearer). لا تسجيل إنتاجي للتوكن.
- **التجربة السلبية (أ)** (إزالة شرط `uid = ?` من `unregister` بتغييره إلى `AND (uid = ? OR 1)`):
  `test devices::tests::unregister_removes_only_own_token ... FAILED` مع
  `panicked at src/devices.rs:696: another user's token must not be removed`.
  أُعيد الملف وتحقّقت `sha256 = 89252afaaf5f688d6d9599077c72bff0b853c08a5fc1bc60bfc1c277edd59fe9` قبل التجربة وبعدها.
- **التجربة السلبية (ب)** (حذف `let _guard = self.write_lock.lock().await;`): شُغّلت 5 مرات ⇒ **`ok` في الخمس كلها**، أي أنها **لم تفشل**. السبب الفعلي: SQLite يسمح بكاتب واحد لكل معاملة، فـ`BEGIN` + كتابة `upsert` تحجز قفل الكتابة وتُسلسل المعاملات حتى بدون القفل داخل العملية؛ لذلك العدّ والإخلاء لا يتداخلان. قوّيت الاختبار إلى 50 مهمة متزامنة (بدل 10) مع التحقق من أن مجموع `evicted` = 47 بالضبط ومن بقاء 3 صفوف، وأعدت التجربة 5 مرات ⇒ `ok` أيضاً. أبقيت القفل لأنه مطلوب حرفياً في الخطوة (حماية داخل العملية وانتظار منظّم)، وليس لأن SQLite يحتاجه للصحة. أُعيد الملف وتحقّقت `sha256` النهائي `89252afa...fe9`.
- ملاحظة على (أ): النسخة الأولى من الاختبار كانت تفحص إزالة التوكن الخاص فقط، فمرّت رغم إزالة الشرط؛ أضفت محاولة إزالة توكن مستخدم آخر قبل الإزالة الصحيحة، فأصبحت التجربة تفشل كما هو مطلوب.

## 4. طريقة الفحص: كل بند من "طريقة الفحص" ← النتيجة

| الفحص المطلوب | النتيجة وكيف تحققت |
|---|---|
| تسجيل جهاز | `register_with_valid_jwt_identity_succeeds`: مزود JWT HS256 ببيئة وهمية وإعداد محفوظ في `ConfigStore` ⇒ 200 و`registered:true`، والصف موجود في المتجر. |
| upsert | `register_inserts_and_updates_last_seen`: بعد تسجيل ثانٍ تغيّر `platform` إلى `ios` وتغيّر `last_seen` عن القيمة القديمة، وبقي صف واحد. |
| نقل الملكية | `register_same_token_transfers_ownership`: توكن واحد انتقل من `uid-1` إلى `uid-2` واختفى من الأول. |
| تجاوز الحد يحذف الأقدم | `register_enforces_max_per_uid_evicting_oldest` (evicted=1 وحُذف الأقدم) و`register_never_evicts_the_new_device` (max=1 مع 3 قديمة ⇒ evicted=3 وبقي الجديد). |
| القصّ 1..=20 | `max_per_uid_is_clamped`: 0 ⇒ صف واحد، 1000 ⇒ 20 صفاً بلا خطأ. |
| تزامن | `concurrent_registers_respect_limit`: 50 مهمة، كلها `Ok`، 3 صفوف، ومجموع الإخلاء 47. |
| بلا توكن هوية ⇒ 401 | `register_without_token_is_401`. |
| مفتاح خدمة لا يسجّل | `service_key_cannot_register_devices` ⇒ 401 حتى مع `SERVICE_API_KEY` مهيأ. |
| تطبيق مجهول ⇒ 404 | `register_unknown_app_is_404`. |
| حقل مجهول ⇒ 400 | `register_unknown_field_is_400` (تحويل رفض `Json` إلى 400). |
| حد 20/دقيقة لكل uid | `register_rate_limit_per_uid_is_429`: 20 طلباً 200 ثم الحادي والعشرون 429. |
| عدم كشف التوكن | `register_response_never_echoes_token` و`token_never_appears_in_error_text` (رسائل الأخطاء عامة). |
| إلغاء لا يميّز الملكية | `unregister_foreign_token_reports_not_removed`: توكن مالك آخر ⇒ 200 و`removed:false` وبقي التوكن. |
| مخبأ الأدوار | `role_uids_cached_within_ttl` (استدعاء واحد للمصدر)، `role_uids_zero_ttl_not_cached` (استدعاءان وبلا مدخلات)، `role_uids_cache_invalidated_by_config_version` (إصدار جديد ⇒ استدعاء جديد). |
| فشل المصدر 503 وغير مخبَّأ | `role_uids_source_error_is_unavailable_and_not_cached` ⇒ `Unavailable` ثم نجاح بعد إصلاح المصدر باستدعاء ثانٍ. |
| تحقق قيمة الدور | `role_value_validation_rejects_injection_like_input`: 6 قيم مرفوضة و0 استدعاء للمصدر. |
| حد المخبأ 256 | `role_cache_is_bounded`: 300 قيمة ⇒ العدد ≤ 256 و≥ 1. |
| غياب `role_members` | `tokens_for_role_requires_role_members_config` ⇒ `NotConfigured` بلا استدعاء مصدر. |
| idempotent | `migration_is_idempotent`. |
| `tokens_for_uids` | `tokens_for_uids_dedups_chunks_and_caps`: 60 uid (دفعتان) + 10 مكررة ⇒ 60 توكن فريد، وسقف 10 ⇒ 10. |

## 5. انحرافات عن الأمر (وسببها)

1. **`src/admin/mod.rs` خارج القائمة المسموحة**: إضافة حقلي `AppState` كسرت تهيئة `test_state()` في اختبارات T2 (`error[E0063]: missing fields devices and devices_limiter`). التعديل الإلزامي: `db: pool` ⇒ `db: pool.clone()`، وسطران لحقلي `devices`/`devices_limiter` داخل `test_state()` فقط. لا تغيير في أي مسار أو منطق إداري. بدونه لا يُبنى المشروع.
2. **دلالة "تطبيق مجهول"**: `ensure_known_app` يعتبر التطبيق معروفاً إذا كان له إعداد محفوظ **أو** ملف credentials في `project_map` (نفس منطق `AuthService::configured_provider`). التطبيق الغائب عن الاثنين ⇒ 404 بنفس رسالة `topics.rs`. هذا يمنع رفض تطبيق JWT مُعدّ بلا ملف service account (وهو حالة مشروعة بعد T3)، ويُبقي 404 للتطبيق المجهول فعلاً.
3. **`authenticate_identity`**: تفويض مباشر إلى `verify_identity` الخاصة بدل تكرار منطق التحقق؛ الدالة الجديدة عامة وتعيد `Identity` كاملة، ولم يتغيّر سلوك أي دالة قديمة.
4. **`#[allow(dead_code)]`**: على `tokens_for_uid`/`tokens_for_uids`/`remove_dead_token`/`tokens_for_role`/`RoleDirectory` لأنها تُستعمل من T8/T9 وليست مربوطة بمسار بعد (لا إرسال في T6). حقلا `AppState.identity`/`datasource` بقيا بـ`#[allow(dead_code)]` لأنهما ما زالا غير مستعملين مباشرة (T6 يستعمل `AuthService`). هذا امتداد لقرار T5 المقبول.
5. **تقسيم الـcommits**: لأن `git add` بالاسم فقط (RULES §3)، مرّ `src/devices.rs` بثلاث حالات: store فقط (15 اختباراً)، ثم + directory (22)، ثم + اختبارات HTTP (30). الحالات الثلاث تُبنى وتنجح اختباراتها الفرعية.
6. **التجربة (ب) لم تفشل**: موثّق أعلاه بالسبب؛ الاختبار قُوّي إلى 50 مهمة + تحقق مجموع الإخلاء، وأبقيت القفل تنفيذاً حرفياً للخطوة.
7. **الاختبار (أ) احتاج تقوية**: النسخة الحرفية للاختبار لم تكن تكشف إزالة الشرط؛ أضفت محاولة إزالة توكن مالك آخر إليه (دون تغيير اسمه) ليطابق الغرض من التجربة.

## 6. ملاحظات خارج النطاق

- لم أُحدّث قائمة `available_endpoints` في `api_fallback` بالمسارين الجديدين (غير مطلوب في T6).
- `tokens_for_role` و`RoleDirectory` غير مربوطين بأي مسار (لا إرسال في T6).
- ملاحظة T4 ما زالت قائمة: `validate_data_source` يقبل `http://localhost.evil.com` بينما المصدر وقت التشغيل يرفضه.
- صفّا T2/T4 في §9 من الخطة ما زالا `🔄` رغم أن `execution/README.md` يعلّمهما ✅؛ لم أعدّلهما (خارج نطاق T6) وأضفت صفّي T5/T6 فقط.
- `PUBLISHED_COMMIT` في `S0-report.md` ما زال `<يملؤه المالك>`، و`c303d3` غير موجود في أي من المستودعين.
- `taqati` لم يُلمس.
- `authenticate_user` في `auth.rs` بقي بـ`#[allow(dead_code)]`؛ T6 أضاف `authenticate_identity` بدلاً من تغيير سلوكه.

## 7. أسئلة للمالك/المراجع

1. هل تعديل `src/admin/mod.rs` الإلزامي داخل `test_state()` مقبول ضمن T6؟ (بدونه لا يبنى المشروع، ولم أجد بديلاً دون تغيير حقول `AppState`.)
2. هل دلالة "معروف = إعداد أو credentials" في `ensure_known_app` موافقة للخطة، أم تفضّل اشتراط `project_map` فقط كما في `topics.rs` حرفياً؟
3. نتيجة التجربة (ب): هل تقبل تفسير تسلسل SQLite مع إبقاء القفل، أم تطلب خطاف اختبار داخل `register` لإثبات أثر القفل؟
4. هل نضيف المسارين إلى `api_fallback` في خطوة لاحقة (T13 توثيق المسارات)؟

## 8. حالة git النهائية

`git status --short` قبل commit التوثيق:
```
 M docs/plans/execution/README.md
 M docs/plans/secure-sending-plan.md
?? docs/plans/execution/T6.md
?? docs/plans/reports/T6-report.md
```
`git log --oneline -5`:
```
51126d3 feat(devices): T6-3 add register and unregister endpoints
c1c547a feat(devices): T6-2 add role directory and tokens_for_role
4a7f1a4 feat(devices): T6-1 add devices table and DeviceStore
257e38e docs(plan): T5 update execution tracker
5d74c08 feat(auth): T5-3 role from config and web_subscribe patterns, keep legacy defaults
```

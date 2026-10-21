# تقرير T5
- التاريخ/الوقت: 2026-10-21 14:09 +03
- الفرع و HEAD قبل/بعد: `feature/policy-auth`، قبل `e437ead`، بعد `5d74c08` (وقبله commit التوثيق).

## 1. ما نُفِّذ (بند بند من ملف الخطوة)

| البند | الحالة | ملاحظة |
|---|---|---|
| §0 التعديلات المسموحة | تم | جديد `src/roles.rs`، وتعديل `src/auth.rs` و`src/main.rs` و`src/admin/mod.rs` داخل `test_state()` + سطر استيراد واحد إلزامي (حُذف منه اسمان محذوفان). `src/topics.rs` لم يحتج تعديلاً: توقيع `authorize_topic` بقي كما هو. |
| §1 `RoleResolver` والمخبأ | تم | `src/roles.rs`: مفتاح `(app, version, uid)`، TTL = `cache_seconds`، 0 ⇒ بلا تخزين، حد 4096 ثم تنظيف المنتهي ومسح الكل، لا قفل عبر `await`، الأدوار `None` تُخزَّن، الأخطاء لا تُخزَّن. |
| §1 `IdentityPolicy` | تم | alias مباشر لـ`config::Identity`؛ سياسة التطبيق بلا إعداد = `AppConfig::default_firebase().identity` (Firebase + `users/{uid}.role` + `["admin"]`). |
| §1 الدور من claim | تم | `claim_at` + نص غير فارغ فقط؛ المصفوفات/الأرقام/الكائنات/الفارغ ⇒ `None` (موثّق بتعليق). بلا تخزين (توكنان لنفس uid قد يختلفان). |
| §1 الدور من مصدر البيانات | تم | `doc.replace("{uid}", uid)` ثم `get_doc`؛ `None` ⇒ `None`؛ `field` مسار نقطي؛ أي `DataError` ⇒ `RoleError::Unavailable("role lookup failed")` مع `tracing::warn!` بالتفصيل وبلا uid. |
| §1 `is_admin_role` | تم | مطابقة حرفية حساسة لحالة الأحرف؛ قائمة فارغة ⇒ false. |
| §1 `topic_matches` | تم | مساواة تامة بلا `*`؛ `*` في النهاية فقط مع لاحقة غير فارغة `[A-Za-z0-9_-]`؛ `*` وحدها أو في الوسط ⇒ false. |
| §2 `AuthService::new` | تم | `new(project_map, identity, datasource, config, auth_config)` + `RoleResolver` داخلياً. |
| §2 الحذف | تم | حُذف `FirebaseVerifier` المحلي و`FirestoreReader`/`FirestoreHttp`/`role_is_admin`/`admin_cache`/`encode_path_segment`/`unverified_audience`/الاستيرادات الميتة. `grep` على الأسماء المحذوفة في `src` ⇒ صفر. |
| §2 `authenticate_admin` | تم | مفتاح الخدمة كما هو؛ `provider_for` ⇒ `verify` ⇒ الدور ⇒ `is_admin_role` وإلا `Forbidden("the user is not an admin for this app")`؛ 401/403/503/404 محفوظة (404 عبر فحص صريح لغياب الإعداد و`project_map`). |
| §2 `authenticate_send` | تم | منطق الخدمة/المفتاح القديم بلا تغيير. |
| §2 `authenticate_user` | تم | عبر المزود؛ أي فشل ⇒ `None`. أُضيف `#[allow(dead_code)]` (انظر §5). |
| §2 `is_admin_identity` | تم | التوقيع الجديد `is_admin_identity(app, &Identity)`؛ لا مستدعي خارجي قديم، ويُستعمل في `authorize_topic` والاختبارات. |
| §2 `authenticate_admin_any` | تم | المرشحون من `project_map` فقط (لا توجد `ConfigStore::app_names()`، والخيار البديل مسموح في الخطوة): Firebase/بلا إعداد عبر `aud`، وJWT عبر `iss` المضبوط. التحقق يتم قبل أي دور، وبحد أقصى 8 مرشحين أبجدياً مع `warn` عند الاقتطاع. |
| §2 `authorize_topic` | تم | مفتاح الخدمة ⇒ Admin؛ هوية صالحة ⇒ `anonymous ∪ signed_in` ⇒ User؛ وإلا دور أدمن ⇒ Admin؛ وإلا 403. بلا هوية ⇒ anonymous فقط. فشل الدور يُرجَع فقط عند الحاجة للدور (بعد فشل مسار User). |
| §2 `topic_allowed` | تم | `Option<&WebSubscribe>`؛ `None` = السلوك القديم حرفياً، `Some(ws)` = الأنماط. `LEGACY_SIGNED_IN_TOPICS` ثابت مسمّى، والقيم الثلاثية لم تبقَ في أي مكان آخر. |
| §2 `AppState` | تم | أُضيف `identity` و`datasource`؛ بُنيا في `main()` قبل `AuthService` ومُرِّرا له. `#[allow(dead_code)]` على الحقلين (يُستعملان في T6 — انظر §5). |
| §2 `admin/mod.rs` | تم | داخل `test_state()` فقط + سطر الاستيراد (إلزامي). لا تغيير في أي مسار أو منطق إداري. |
| §3 اختبارات roles | تم | 15/15 بالأسماء المطلوبة. |
| §3 اختبارات auth | تم | الأسماء الجديدة كلها موجودة؛ القديمة كُيِّفت (التفاصيل في §4/§5). |
| §4 الجودة والتجربتان | تم | `build` نظيف، `clippy --all-targets` صفر، `test` 212/0/0، والتجربتان السلبية نجحتا (تفاصيل §3/§4). |
| §5 التوثيق والـ commits | تم | 3 commits كود + commit التوثيق هذا. |

## 2. الملفات
`git diff --stat e437ead..HEAD -- src`:
```
 src/admin/mod.rs |   32 +-
 src/auth.rs      | 1531 ++++++++++++++++++++++++++++-------------------
 src/main.rs      |   59 ++-
 src/roles.rs     |  568 ++++++++++++++
 4 files changed, 1644 insertions(+), 546 deletions(-)
```
- `src/roles.rs` (جديد، 568 سطراً): المحلّل + المخبأ + `is_admin_role` + `topic_matches` + `legacy_policy` + `NoSource` + 15 اختباراً.
- `src/auth.rs` (1698 سطراً): `AuthService` فوق السجلين، الدور من الإعداد، `topic_allowed` بالأنماط، `authenticate_admin_any` بالمرشحين، والاختبارات مكيَّفة/موسَّعة.
- `src/main.rs`: بناء `IdentityRegistry`/`DataSourceRegistry` بـ`SystemEnv`، حقلا `AppState`، `mod roles;`، و`redirect::Policy::none()` على عميل HTTP (انظر §5).
- `src/admin/mod.rs`: داخل `test_state()` فقط + حذف اسمين من سطر استيراد الاختبارات.
- `docs/plans/execution/T5.md` (كان غير متتبعاً فأُضيف)، `docs/plans/execution/README.md`، `docs/plans/reports/T5-report.md`.

## 3. الأوامر التي شغلتها ونتائجها
- `cargo build` ⇒ `Finished dev profile` بلا تحذير.
- `cargo clippy --all-targets` ⇒ `Finished` بلا أي تحذير.
- `cargo test` ⇒ `test result: ok. 212 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out` (الأساس كان 187).
- `cargo test -- --list | grep -c ": test$"` ⇒ `212`.
- `cargo test -- --list` للجديد: 15 اختبار `roles::tests::*` و10 أسماء جديدة في `auth::tests::*` (إضافة إلى `non_admin_role_is_forbidden` و`role_lookup_failure_is_503_not_403` الموجودَين بالتكييف/التسمية).
- `rustfmt --edition 2021 --config skip_children=true src/main.rs src/auth.rs src/roles.rs` ثم `--check` ⇒ لا فرق.
- فحص العربية: `perl -CSD -ne '...' $(find src examples ...)` مع استثناء `validation.rs` و`topics.rs` ⇒ صفر سطر.
- `grep -rn "FirestoreHttp\|FirestoreReader\|role_is_admin" src` ⇒ لا شيء.
- `grep -rn "std::env::var" src/roles.rs src/auth.rs` ⇒ لا شيء.
- `head -n <سطر #[cfg(test)]>` ثم `grep "unwrap(\|expect("` على `roles.rs`/`auth.rs` ⇒ لا شيء خارج الاختبارات.
- التجربة السلبية (أ): `candidate == role` ⇒ `candidate.eq_ignore_ascii_case(role)` ⇒
  `thread 'roles::tests::admin_role_match_is_exact_and_case_sensitive' panicked ... assertion failed: !is_admin_role(Some("Admin"), &admin_roles)` ثم أُعيد الملف.
- التجربة السلبية (ب): `let key = (app.to_string(), 0, ...)` ⇒
  `thread 'roles::tests::role_cache_invalidated_by_config_version' panicked ... assertion left == right failed` ثم أُعيد الملف.
- `sha256` قبل/بعد: `roles.rs = 5b3fe19c06b50cc4ae72d70861aabf05919f8969f610f0622d84da0ad683c22f`، `auth.rs = f84f8763f75ae7b4570a6fd741b8424f3c2d96399921f2cc1d4b8694f51ebd06` — متطابقان قبل التجربتين وبعدهما.

## 4. طريقة الفحص: كل بند من معايير القبول
1. **النطاق**: `git diff --stat e437ead -- src/identity src/datasource src/config src/validation.rs src/credentials.rs Cargo.toml Cargo.lock dashboard policies` ⇒ فارغ. `src/admin/mod.rs` الفرق كله داخل `mod tests`/`test_state()` (سطر الاستيراد + بناء السجلين + الحقلين). `git diff --name-only e437ead..HEAD` ⇒ `src/roles.rs` (جديد) و`auth.rs` و`main.rs` و`admin/mod.rs` فقط.
2. **القيم الثلاثية**: `grep -n "users\|companies\|technicals" src/auth.rs src/topics.rs` ⇒ فقط `LEGACY_SIGNED_IN_TOPICS` والتعليقات والاختبارات. `topics.rs` لم يتغير.
3. **الأسماء المحذوفة**: `grep -rn "FirestoreHttp\|FirestoreReader\|role_is_admin" src` ⇒ صفر.
4. **الجودة**: لا `unwrap/expect` خارج الاختبارات في `roles.rs`/`auth.rs`؛ لا `std::env::var` فيهما؛ فحص العربية صفر؛ `clippy --all-targets` صفر.
5. **الاختبارات**: كل الأسماء المطلوبة موجودة (`--list` أعلاه). التكييف لم يُضعف أي assert:
   - اختبارات التوكن القديمة (Firebase/RS256) صارت HS256 عبر `IdentityRegistry` + `EnvSource` وهمي، مع نفس تأكيدات 401/403/503 ونفس الحالات (منتهي، iss/aud خاطئان، توقيع بسر آخر، alg=none، iat مستقبلي، sub فارغ).
   - `unknown_kid_refreshes_keys_once_then_throttles` وفحص `auth_time` المستقبلي بقيا كما هما لكن على `FirebaseVerifier` مباشرة (نفس التأكيدات الثلاثة: قبول kid المدوَّر بعد refresh، عدّ النداءات 1→2→2، ورفض `Invalid`)، لأن `AuthService` لم يعد يملك verifier.
   - `firestore_failure_fails_closed_with_503` أُعيدت تسميته `role_lookup_failure_is_503_not_403` بنفس التأكيد (`Unavailable` على فشل المصدر) بعد حذف `FirestoreHttp`.
   - `encodes_firestore_path_segments` بقي بأسمائه الأربعة وتوقعاته الأربعة نفسها لكن على `datasource::percent_encode` (نفس الترميز) مع تأكيدات `is_valid_uid`.
6. **التقرير**: هذا الملف يذكر الانحرافات واختيار المرشحين ونتيجتي التجربتين.

## 5. انحرافات عن الأمر (وسببها)
- **`#[allow(dead_code)]` جديد في ثلاثة مواضع**: حقلا `AppState.identity`/`datasource` (الخطوة تطلب إضافتهما وتقول إنهما يُستعملان في T6؛ بدونهما يُطلق rustc تحذير «field is never read»)، و`authenticate_user` (الخطوة §2 تطلب إبقاءه، ولا مستدعي حي له بعد الآن لأن `authorize_topic` يحتاج الهوية كاملة). لا يوجد حل بلا تحذير أو بلا تغيير سلوك؛ أُبقيت التحذيرات صفر.
- **`src/admin/mod.rs`**: حُذف اسمان من سطر استيراد الاختبارات (`FirestoreHttp`/`GoogleJwkProvider`) لأنهما لم يوجدا بعد الحذف، وهذا خارج `test_state()` نصاً لكنه إلزامي للترجمة. لا تغيير آخر.
- **`main.rs`: `redirect::Policy::none()`** أُضيف لعميل HTTP العام. عقد T4 يفترض عميلاً بهذه السياسة (تعليق `HttpDataSource::new`)، و`main` لم يكن يضبطها؛ بدونها كان resolver الإنتاجي قد يتبع تحويلاً خارج المسار الموقّع. تغيير سطر واحد في ملف مسموح.
- **تقسيم commits**: commit 2 يحمل ربط السجلين **مع** الدور من الإعداد (لأنهما لا يُترجمان منفصلين بلا كود ميت)، وcommit 3 يكمل سياسة الاشتراك (`web_subscribe`) واختباريها ويحفظ الافتراضات القديمة.
- **`unconfigured_app_keeps_legacy_admin_behavior`**: لا يمكن التحقق فيه من توكن Firebase فعلي بلا شبكة، لأن `IdentityRegistry` يبني `GoogleJwkProvider` داخلياً ولا حقن لـ`JwkProvider` (تعديل `src/identity/**` ممنوع). لذلك يثبت: `version == 0`، وأن السياسة تساوي `AppConfig::default_firebase().identity` (مزوّد Firebase، `users/{uid}`/`role`/60، `admin_roles=["admin"]`)، ثم يشغّل `RoleResolver` بمصدر وهمي يطبّق `DataSource`: وثيقة `role=admin` ⇒ أدمن، ووثيقة مفقودة ⇒ ليس أدمن. مسار Firebase نفسه مغطى باختبارات `identity` + اختباري verifier أعلاه.
- **`authenticate_admin_any` والمرشحون**: `ConfigStore` لا يوفر تعداد أسماء التطبيقات (فيه `app_count`/`get` فقط)، فاعتمدت الخيار البديل الذي تسمح به الخطوة: `project_map` فقط. أثر ذلك: تطبيق له إعداد بلا credentials لا يمكن أن يكون مرشحاً، لكنه أصلاً لا يستطيع الإرسال.
- **`web_subscribe` دائماً struct في `AppConfig`**: أي تطبيق له إعداد ولم تُضبط `web_subscribe` يحصل على قائمتين فارغتين ⇒ كل المواضيع ممنوعة عنه حتى يُضبط (وهذا نصّ الخطوة: «حتى بقائمة فارغة»). التطبيقات بلا إعداد لم تتأثر.
- **مسارات `RoleSource::DataSource`**: التحقق الفعلي من المسار يحدث داخل `validate_doc_path` في المصدر؛ مسار غير صالح يعطي `InvalidPath` فيُحوَّل إلى `Unavailable` (503 fail-closed) وليس 400. `V4` يتحقق فقط من أن `doc` غير فارغ. لم ألمس `src/config/**`.

## 6. ملاحظات خارج النطاق
- من T4 وما زال قائماً: `validate_data_source` في `src/config/validate.rs` يقبل `http://localhost.evil.com` (فحص بادئة)، بينما `HttpDataSource::validate_base_url` يرفضه وقت التشغيل.
- `src/topics.rs` لم يحتج أي تعديل: `authorize_topic` حافظ على توقيعه. لا عربية مضافة.
- `docs/plans/reports/S0-report.md` لا يزال `PUBLISHED_COMMIT` بـ`<يملؤه المالك>`.
- `RoleResolver` يخزّن `None` سلبياً بنفس TTL (مطلوب)، لكن تغيير دور المستخدم في المصدر لا يظهر قبل انتهاء TTL أو تغيير إصدار الإعداد.

## 7. أسئلة للمالك/المراجع
1. هل تُقبل `#[allow(dead_code)]` الثلاثة (حقلان لـT6 + `authenticate_user` المطلوب في §2) كبديل وحيد عن تحذير build أو كود ميت؟
2. هل تُقبل إعادة اختبارات Firebase الخاصة (تحديث JWKS و`auth_time`) إلى مستوى `FirebaseVerifier` داخل `auth.rs` بدل حذفها، مع بقاء تأكيداتها حرفياً؟
3. هل تُقبل إعادة تسمية `firestore_failure_fails_closed_with_503` إلى `role_lookup_failure_is_503_not_403` (نفس التأكيد)؟
4. هل `redirect::Policy::none()` على عميل `main` مقبول ضمن T5 (تفعيل عقد T4) أم يُؤجَّل لخطوة لاحقة؟
5. تأكيد سلوك `web_subscribe` الفارغة ⇒ منع كل المواضيع لتطبيق له إعداد (وفق نص الخطوة).
6. ما زال مفتوحاً: `PUBLISHED_COMMIT` في S0، تغطية ES256، رفض `alg=none` عند رأس التوكن، صيغة سجل اسم متغير السر، وتمديد `reauth` لـ`expires_at` (T10).

## 8. حالة git النهائية
```
$ git status --short
(نظيف بعد commit التوثيق)
$ git log --oneline -5
<HEAD> docs(plan): T5 update execution tracker
5d74c08 feat(auth): T5-3 role from config and web_subscribe patterns, keep legacy defaults
9da9695 refactor(auth): T5-2 use identity and data source registries in AuthService
c82b897 feat(roles): T5-1 add role resolver, cache and topic pattern matching
e437ead docs(plan): T4 update execution tracker and resolver contract
4e36dc8 feat(datasource): T4-3 add data source registry
```

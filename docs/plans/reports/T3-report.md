# تقرير T3

- التاريخ/الوقت: 2026-10-21 13:23 +03
- الفرع و HEAD قبل/بعد: `feature/policy-auth` — قبل: `8619e23` — بعد الكود: `d37d23a` (يليه commit الوثائق الذي يحمل هذا الملف)
- النطاق المنفَّذ: الجزء 1 (إصدارات القوالب) ثم الجزء 2 (نقل JWKS/Firebase) ثم الجزء 3 (الواجهة + مزود JWT + السجل) ثم التوثيق. 33 اختباراً جديداً، المجموع 152.

## 1. ما نُفِّذ (بند بند من ملف الخطوة)

| البند | الحالة | ملاحظة |
|---|---|---|
| §1 `import` يجعل إصدارات القوالب متصاعدة | تم | لقالب موجود: `إصداره السابق + 1`، ولجديد: 1. لا يمسح `config_versions` (تاريخ القالب المحذوف يبقى). commit `53971d8` |
| §1 اختبار `import_keeps_template_versions_monotonic` | تم | استيراد مرتين: 1 ثم 2، ثم `save_template(..., expected_version=Some(1))` ⇐ `Conflict { current_version: 2 }` |
| §1 رسالة الـ commit | تم | `fix(config): T3-0 keep template versions monotonic across import` |
| §2 نقل `JwkProvider`/`FetchedJwks`/`JwksCache` وسياسة التحديث إلى `identity/jwks.rs` | تم | `MIN_JWK_REFRESH`، `max-age` بين 60ث و6س، مجهول ⇒ تحديث واحد/دقيقة، انتهاء + خنق ⇒ `Expired` (تتحول عند `auth` إلى `Unavailable`) |
| §2 `HttpJwkProvider::new(client, url)` + `GoogleJwkProvider::new(client)` كمنشئ يمرّر عنوان Google | تم | `GoogleJwkProvider` غلاف حول `HttpJwkProvider` بنفس العنوان (يحتفظ بمهلة 10ث لكل طلب كما كان) |
| §2 نقل تحقق Firebase إلى `FirebaseVerifier` | تم | `identity/firebase.rs`؛ نفس الرسائل والشروط حرفياً |
| §2 `AuthService` بنفس الواجهة والتواقيع | تم | `AuthService::new(project_map, jwks, firestore, config)` كما هو؛ `pub use` للـ`GoogleJwkProvider`/`JwkProvider`/`FetchedJwks`/`FirebaseVerifier` |
| §2 اختبارات `auth.rs` القديمة بلا تعديل `assert` | تم | `git diff -U0 src/auth.rs` على أسطر `assert` = فارغ. فقط أسطر `use` في وحدة الاختبار تغيّرت |
| §2 `git diff` لـ`auth.rs` ينقص أسطراً أكثر مما يضيف | تم | ‎−270 / +33 |
| §2 رسالة الـ commit | تم | `refactor(identity): T3-1 extract JWKS cache and Firebase verifier from auth.rs` |
| §3 `Identity`/`IdentityError`/`IdentityProvider`/`claim_at` | تم | `identity/mod.rs` |
| §3 تحويل uid: نص غير فارغ ≤128 بلا أحرف تحكم، عدد صحيح ⇒ نص، غير ذلك `Invalid("token has no usable uid")` | تم | `uid_from_claim` |
| §3 `FirebaseProvider { project_id, verifier }` ينفّذ `IdentityProvider` | تم | يرجع uid + email من التوكن، `name = None`، والـ claims كاملة |
| §3 `JwtProvider::new(config, env, jwks)` + `EnvSource` | تم | تنفيذ حقيقي `SystemEnv` + تنفيذ وهمي في الاختبارات (لا تعديل لبيئة العملية) |
| §3 قواعد `verify` 1–8 (الطول، alg المثبت، kid، الفحوص، claims، الكاش، عميل JWKS، عدم التسجيل) | تم | التفاصيل في §4؛ `none` مرفوض (يُرفض أثناء قراءة الترويسة لأن `jsonwebtoken 11` لا يعرف `none`) |
| §3 `IdentityRegistry::new(...)` و`provider_for` والمخبأ لكل `(app, config.version)` وسلوك التطبيق المجهول | تم | `identity/registry.rs`؛ إعادة البناء عند تغيّر الإصدار، حذف نسخ التطبيق القديمة من المخبأ |
| §3 إضافة `identity: Arc<IdentityRegistry>` إلى `AppState` | **لم يتم** | تعارض: مُهيّئ `AppState` في اختبارات `src/admin/mod.rs` يجب أن يتغيّر، و`src/admin/**` ممنوع في §0 وفي معيار القبول 1. لم ألمس الملف؛ الشرح في §5، والسؤال 1 في §7 |
| §4 أسماء الاختبارات المطلوبة | تم | 33 اختباراً بالاسم المطلوب (القائمة في §3) |
| §4 ES256 | تم ترجمة الدعم، **بلا اختبار** | لا توجد طريقة لتوليد مفاتيح EC بلا اعتمادية جديدة (`p256`)؛ مذكور كـ«غير مغطى» |
| §5 `cargo fmt` على ملفاتي فقط | تم | لم أُنسّق `credentials.rs` (استُعيد بايت-بايت بعد أن جرّه `rustfmt src/main.rs` ثم أُعيد التحقق) |
| §5 `cargo build`/`clippy --all-targets` بصفر تحذير | تم | §3 |
| §5 تجربة تعطيل فحص `alg` يدوياً | تم | §4، مع إعادة الملف مطابقاً (sha256 متساوٍ) |
| §6 README: T2=✅ بقراراتها و T3=🔄 | تم | القراران: CORS عبر proxy في Vite ضمن T12؛ `internal` معتمد |
| §6 الخطة: `internal` في 3.15 + سطر Vite في T12 + سطر `X-Forwarded-Proto` في T13 + سطر سجل التقدم | تم | |
| §6 إضافة `T3.md` والتقرير | تم | |

## 2. الملفات (`git diff --stat 8619e23..HEAD`)

```text
 src/auth.rs              | 302 ++-----------------
 src/config/store.rs      |  59 +++-
 src/identity/firebase.rs | 271 +++++++++++++++++
 src/identity/jwks.rs     | 285 ++++++++++++++++++
 src/identity/jwt.rs      | 740 +++++++++++++++++++++++++++++++++++++++++++++++
 src/identity/mod.rs      | 159 ++++++++++
 src/identity/registry.rs | 367 +++++++++++++++++++++++
 src/main.rs              |   1 +
 8 files changed, 1913 insertions(+), 271 deletions(-)
```

الملفات الجديدة (سطر لكل ملف):

| الملف | الشرح |
|---|---|
| `src/identity/mod.rs` | `Identity`, `IdentityError`, `IdentityProvider`, `claim_at`, قواعد uid، حدود الطول |
| `src/identity/jwks.rs` | `JwkProvider`, `FetchedJwks`, `JwksCache` وسياستها، `HttpJwkProvider` (64KB، https، بلا توجيه)، `GoogleJwkProvider`، `UnconfiguredJwkProvider`، `parse_cache_max_age` |
| `src/identity/firebase.rs` | `FirebaseVerifier` (نقل حرفي) + `FirebaseProvider` + اختبارا Firebase |
| `src/identity/jwt.rs` | `EnvSource`/`SystemEnv`، `JwtProviderConfig`، `JwtProvider`، و22 اختبار JWT |
| `src/identity/registry.rs` | `IdentityRegistry` (بناء/تخزين المزودين) + 6 اختبارات |

## 3. الأوامر التي شغّلتها ونتائجها

```text
$ cargo build
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.15s     # 0 warnings

$ cargo clippy --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.04s     # 0 warnings

$ cargo test
test result: ok. 152 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 22.78s

$ cargo test -- --list | grep -c ": test$"
152
```

توزيع العدد: 118 (قبل T3) + 1 (`import_keeps_template_versions_monotonic`) + 33 (identity) = 152. لا `#[ignore]` (`grep -rn "#\[ignore\]" src` = 0).

أسماء اختبارات `identity` (33، من `-- --list`):

```text
identity::firebase::tests::firebase_provider_rejects_wrong_audience
identity::firebase::tests::firebase_provider_returns_identity_with_email
identity::jwt::tests::jwt_alg_none_rejected
identity::jwt::tests::jwt_audience_array_accepted
identity::jwt::tests::jwt_expired_rejected
identity::jwt::tests::jwt_future_iat_rejected
identity::jwt::tests::jwt_header_supplied_key_is_ignored
identity::jwt::tests::jwt_hs256_missing_secret_env_is_unavailable
identity::jwt::tests::jwt_hs256_token_against_rs256_provider_rejected
identity::jwt::tests::jwt_hs256_valid_accepted
identity::jwt::tests::jwt_jwks_fetch_failure_is_unavailable
identity::jwt::tests::jwt_missing_kid_uses_sole_key
identity::jwt::tests::jwt_missing_uid_claim_rejected
identity::jwt::tests::jwt_nested_uid_claim
identity::jwt::tests::jwt_numeric_uid_claim_converted
identity::jwt::tests::jwt_older_than_max_age_rejected
identity::jwt::tests::jwt_oversized_token_rejected
identity::jwt::tests::jwt_rs256_token_against_hs256_provider_rejected
identity::jwt::tests::jwt_rs256_valid_token_accepted
identity::jwt::tests::jwt_short_hs256_secret_is_unavailable
identity::jwt::tests::jwt_uid_too_long_rejected
identity::jwt::tests::jwt_unknown_kid_refreshes_once_per_minute
identity::jwt::tests::jwt_wrong_audience_rejected
identity::jwt::tests::jwt_wrong_issuer_rejected
identity::registry::tests::registry_builds_provider_per_config
identity::registry::tests::registry_caches_provider_between_calls
identity::registry::tests::registry_defaults_to_firebase_without_config
identity::registry::tests::registry_firebase_without_credentials_is_unavailable
identity::registry::tests::registry_rebuilds_when_config_version_changes
identity::registry::tests::registry_unknown_app_is_invalid
identity::tests::claim_path_reads_nested_and_arrays
identity::tests::claim_path_rejects_empty_segments
identity::tests::uid_conversion_rules
```

فحوص القبول:

```text
$ grep -rn "std::env::var" src/identity
src/identity/jwt.rs:33:        std::env::var(name).ok().filter(|value| !value.is_empty())   # داخل SystemEnv فقط

$ # Arabic scan (perl -CSD '[\x{0600}-\x{06FF}]' on src minus validation.rs/topics.rs)
0 hits

$ # unwrap()/expect( before #[cfg(test)] in src/identity/*.rs
0 hits
```

## 4. طريقة الفحص: كل بند ← النتيجة الفعلية وكيف تحققت

**1) اختبارات Firebase القديمة:** `cargo test auth::` = 20 ناجحاً بلا أي تعديل `assert` (`git diff -U0 src/auth.rs` على كلمات `assert` = 0 أسطر). سلوك المسارات الحالية لم يتغيّر لأن `AuthService` يحوّل `IdentityError` إلى `AuthError` بنفس الرسائل السابقة.

**2) JWT — التوقيع والقبول:** `jwt_rs256_valid_token_accepted` (مفتاح RSA مولَّد في الاختبار، كما تفعل اختبارات auth) ⇐ `Identity { uid, email, name }`. `jwt_audience_array_accepted` بمصفوفة تحتوي القيمة.

**3) `alg=none`:** `jwt_alg_none_rejected` ⇒ `Invalid`. ملاحظة: `jsonwebtoken 11` لا يملك متغير `none` أصلاً، فترويسة `none` تُرفض أثناء `decode_header`؛ النتيجة نفسها (رفض) لكن الرسالة `malformed token`.

**4) تبديل العائلات:** `jwt_hs256_token_against_rs256_provider_rejected` و`jwt_rs256_token_against_hs256_provider_rejected` ⇐ `Invalid`. **تجربة التعطيل الإلزامية:** عطّلت مؤقتاً `configured_algorithm` لتقبل أي `alg`:

```text
test identity::jwt::tests::jwt_hs256_token_against_rs256_provider_rejected ...
panicked: expected Invalid, got Err(Unavailable("identity secret is not configured"))
test result: FAILED. 0 passed; 1 failed; 151 filtered out
```

أي أن الاختبار يحرس فحص `alg` فعلاً. أُعيد الملف فوراً و`sha256` قبل/بعد متساويان:
`bccb23e…66e5d9db` = `bccb23e…66e5d9db`، والاختبار ناجح بعد الإعادة.

**5) iss/aud/exp/iat:** `jwt_wrong_issuer_rejected`، `jwt_wrong_audience_rejected`، `jwt_expired_rejected` (exp=1000)، `jwt_future_iat_rejected` (iat=الآن+3600)، `jwt_older_than_max_age_rejected` (iat=الآن−3601 و`max_age_seconds=3600`). `nbf` مفحوص عند وجوده بسماحية 60ث (لا اختبار مستقل، مغطى بالمسار).

**6) claim متداخل:** `claim_path_reads_nested_and_arrays` يقرأ `app_metadata.role` و`realm_access.roles` (صفيف)، ويرفض المرور داخل مصفوفة (`realm_access.roles.0` ⇒ `None`). `jwt_nested_uid_claim` مع `uid_claim = "user.id"`.

**7) سر HS256 مفقود ⇐ 503 + سجل:** `jwt_hs256_missing_secret_env_is_unavailable` و`jwt_short_hs256_secret_is_unavailable` (29 بايت) ⇐ `IdentityError::Unavailable("identity secret is not configured")` (يحوّلها T5/T8 إلى 503). السجل في `jwt.rs`:
`tracing::error!(secret_env = ..., "the HS256 identity secret is missing or shorter than 32 bytes")` — اسم المتغير فقط، لا قيمة.

**8) حد الطول و`kid`:** `jwt_oversized_token_rejected` (8193 بايت)، `jwt_missing_kid_uses_sole_key`، `jwt_unknown_kid_refreshes_once_per_minute` (نداءان فقط، والثالث مجهول ⇐ `Invalid` بلا نداء ثالث)، `jwt_jwks_fetch_failure_is_unavailable`، و`jwt_header_supplied_key_is_ignored` (ترويسة `jwk` لمفتاح المهاجم + توقيعه ⇒ مرفوض).

**9) السجل:** `registry_builds_provider_per_config` يثبت أن مزود التطبيق يستعمل سر HS256 المضبوط في إعداداته (توكن صحيح ⇐ قبول). `registry_rebuilds_when_config_version_changes` ⇐ `Arc` مختلف بعد `save_section` وتوكن الجمهور الجديد يُقبل. `registry_caches_provider_between_calls` ⇐ نفس `Arc`. `registry_defaults_to_firebase_without_config` ⇐ مزود افتراضي مخبّأ. `registry_firebase_without_credentials_is_unavailable` ⇐ `"app has no loaded credentials"`. `registry_unknown_app_is_invalid` ⇐ `"unknown app"`.

**10) قواعد uid:** `uid_conversion_rules` (نص، فارغ/مسافات/محرف تحكم/129 حرفاً ⇐ رفض، 42 ⇒ `"42"`، −7 ⇒ `"-7"`، 1.5/null/bool/كائن/مصفوفة ⇐ رفض).

## 5. انحرافات عن الأمر (وسببها)

1. **حقل `identity` في `AppState` لم يُضف (البند الوحيد الناقص).** إضافة الحقل تُبطل تهيئة `AppState` في اختبارات `src/admin/mod.rs` (`test_state()` سطر 308)، وهي ملف ممنوع تعديله بنص §0 ومعيار القبول 1. الاحتمالات كانت: (أ) تعديل سطر واحد في ملف ممنوع، (ب) `cargo build` يفشل، (ج) ترك الحقل. اخترت (ج) التزاماً بـ RULES §1.2 (لا إصلاح خارج النطاق) و§1.3 (توقف عند البند واكتب السبب). النتيجة: `IdentityRegistry` مكتمل ومختبَر لكنه لا يُبنى في `main` بعد. `src/main.rs` تغيّر بسطر واحد فقط: `mod identity;`.
2. **`#![allow(dead_code)]` أعلى `src/identity/mod.rs`.** لأن السجل والمزودين لا يُستعملون من مسار حقيقي بعد (البند 1)، فبدونه يظهر 19 تحذيراً في `cargo build` ويُخالف شرط §5. التعليق يوضح أن T5/T8 هي التي تربطها. سأزيله مع الحقل.
3. **`JwtProviderConfig` مُعرَّف في `identity/jwt.rs`** وليس في `config`، لأن `src/config/mod.rs` ليس ضمن الملفات المسموحة في §0. السجل يحوّل حقول `config::IdentityProvider::Jwt` إليه.
4. **فحص Firebase يقرأ claims كـ`Value` أولاً** ثم `serde_json::from_value::<RawClaims>`، ليبقى `Identity.claims` كاملاً (يحتاجه T5) مع نفس صرامة الأنواع السابقة (بما فيها رفض `sub` غير النصي و`exp` غير الرقمي).
5. **`HttpJwkProvider` يعمّم فلتر المفاتيح إلى RS256 وES256** (بدل RS256 فقط) لأن مزود JWT يحتاج ES256؛ لا أثر على Firebase لأن ترويسة Firebase تُثبَّت RS256 قبل اختيار المفتاح. أُضيف حد 64KB (بالطول إن وُجد وبالتدفق)، وفحص `https://` وقت الطلب.
6. **مهلة Google بقيت 10ث** عبر `HttpJwkProvider::with_timeout`؛ المتطلب «عميل 5ث بلا توجيه» يُنفَّذ عند بناء السجل في `main` (لم يُربط بعد — البند 1). لا توجيه داخل المزود: العميل هو من يقرر.
7. **كود `internal` (500) أُضيف إلى جدول 3.15** كما طلب §6، لكن موضعه قبل 503 ليبقى الترتيب تصاعدياً.
8. **`alg=none` يُرفض أثناء قراءة الترويسة** لا في مقارنة القائمة، لأن `jsonwebtoken 11` لا يملك `none` في `Algorithm`. النتيجة رفض كما هو مطلوب، ورسالة `Invalid`.
9. **`jwt_alg_none_rejected` و`jwt_oversized_token_rejected`** يفحصان الحالة العامة `Invalid` بلا رسالة محددة، حتى لا يتقيّد الاختبار بنص رسالة داخلية.
10. **`rustfmt src/main.rs`** جرّ `src/credentials.rs` مرة أخرى كما في T2؛ استُعيد بـ`git show HEAD:src/credentials.rs > src/credentials.rs` وتأكدت أن `git diff` عليه فارغ، ثم أُعيد تنسيق `main.rs` وحده بعد الاستعادة. `rustfmt --check src/main.rs` يشتكي من `credentials.rs` فقط (حالة سابقة في HEAD، لم ألمسها).
11. **إعادة كتابة `T3-2` محلياً** (`--amend`) بعد إضافة مهلة 10ث لـGoogle وتنظيف سطر فارغ في `main.rs`؛ لا push، والاختبارات أُعيد تشغيلها بعد التعديل (152 ناجحاً).

## 6. ملاحظات خارج النطاق (لم أصلحها)

1. **حقل `AppState` (البند 1 أعلاه):** الحل الأصغر لاحقاً هو سطر واحد في `test_state()` داخل `src/admin/mod.rs` (`identity: Arc::new(IdentityRegistry::new(...))`) مع بناء عميل 5ث بلا توجيه في `main`. حتى يُسمح بذلك، السجل غير موصول.
2. **عربية في ملفين ممنوعين:** `src/validation.rs:198` و`src/topics.rs:229` (بيانات اختبار `"عربي"`). معيار القبول 2 في T3 يستثنيهما صراحةً، والنتيجة 0 على بقية `src`.
3. **ES256 بلا اختبار:** `p256` غير موجود في الاعتماديات، و§0 يمنع إضافة اعتماديات. الدعم مترجم (خوارزمية مسموحة، مفتاح من JWKS) لكنه غير مثبت باختبار.
4. **CORS و`X-Requested-With`:** لم يُعدّل (قرار T3 §6): الحل عبر proxy في Vite ضمن T12، وأُضيف سطر `X-Forwarded-Proto` إلى T13.
5. **`import` وإصدارات القوالب:** سؤال T2 رقم 3 أُغلق بإصلاح T3-0 (إصدار متصاعد بدل 1). أي عميل كان يحمل `expected_version=1` سيحصل على `Conflict` بعد استيراد لاحق — وهو المقصود.
6. **`JwtProvider` يخزّن نسخة من `JwtProviderConfig` بلا `jwks_url` مستعمل** (السجل هو من يبني `HttpJwkProvider` من الرابط)؛ الحقل عام فلا تحذير، لكنه للتوثيق فقط.
7. **`S0`:** `PUBLISHED_COMMIT` ما زال `<يملؤه المالك>`، و`feature/policy-notifications` ما زال غير منشأ — كلاهما من S0، لم أمسّه.
8. **`data.db` وسجلات الأسرار:** لم تُعدّل، ولم يظهر أي `*.db` جديد (اختبارات السجل تستخدم SQLite في الذاكرة).

## 7. أسئلة للمالك/المراجع

1. **`AppState`:** هل أضيف الحقل في إصلاح T3 مع تعديل سطر واحد في `test_state()` داخل `src/admin/mod.rs` (ملف ممنوع)، أم أؤجّل الحقل والربط إلى T5؟ القرار الحالي: مؤجّل، والسجل مبني ومختبَر فقط.
2. **ES256:** أضيف `p256` كاعتمادية تطوير في مهمة لاحقة لكتابة اختبار، أم يبقى «غير مغطى» كما هو موثق؟
3. **`alg=none`:** هل قبول الرفض أثناء قراءة الترويسة (بدل مقارنة القائمة) مقبول للمراجع؟
4. **سجل الخطأ:** هل صيغة `tracing::error!(secret_env = "<name>", ...)` كافية (اسم المتغير فقط)، أم تفضّل صيغة أخرى؟
5. **أسئلة T2 المعلّقة:** لم يبقَ منها إلا `PUBLISHED_COMMIT` في S0؛ بقية القرارات (CORS، `internal`، `reauth`، `import`) حُسمت أو أُرجئت كما في README.

## 8. حالة git النهائية

قبل commit الوثائق:

```text
$ git status --short
 M docs/plans/execution/README.md
 M docs/plans/secure-sending-plan.md
?? docs/plans/execution/T3.md

$ git log --oneline -4
d37d23a feat(identity): T3-2 add JWT provider, interface and provider registry
915689a refactor(identity): T3-1 extract JWKS cache and Firebase verifier from auth.rs
53971d8 fix(config): T3-0 keep template versions monotonic across import
8619e23 docs(plan): T2 update execution tracker
```

لا push. الفرع `feature/policy-auth` تحقّقت منه قبل البدء وقبل كل commit (`git rev-parse --abbrev-ref HEAD` = `feature/policy-auth`).

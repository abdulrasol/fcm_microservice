# تقرير T8

- التاريخ/الوقت: 2026-10-21
- الفرع: `feature/policy-auth`
- HEAD قبل T8: `21c9072` — HEAD بعد T8: `07d9cab`
- الـ commits: `1956267` (T8-1) ← `e4888a2` (T8-2) ← `e3afebc` (T8-3) ← `07d9cab` (T8-4) ← ثم commit الوثائق (T8-5)

## 1. ما نُفِّذ (بند بند من ملف الخطوة)

| البند | الحالة | ملاحظة |
|---|---|---|
| اختبارات توصيف لـ`/send` قبل نقل الكود (commit مستقل) | تم | 4 اختبارات في `src/main.rs`، استُخرجت `raw_send_success_response` و`raw_send_fcm_error_response` بلا تغيير سلوك |
| `src/send.rs`: `FcmTransport` + `HttpFcm` | تم | `Accepted{body}` / `Rejected{http_status, body, dead_token}`، `SendError::{Auth, Network}`، `build_message` يبني `{"message":{...}}` كاملاً |
| `send_to_tokens` بتزامن محدود | تم | `JoinSet` + `Semaphore`، `DEFAULT_CONCURRENCY = 10`، بلا إعادة محاولة، بلا تسجيل توكن، مهمة منهارة = فشل واحد |
| إعادة كتابة `send_notification` بالـtrait بنفس الردود | تم | 502 لتوكن/شبكة، تمرير 4xx من FCM، 500 لغلاف الحساب الغائب، حفظ topic عند النجاح فقط (انظر §5) |
| `AppState`: `fcm` + `role_directory` + `template_rate` | تم | الأربعة تُبنى في `main()` وفي مساعدات الاختبار |
| إزالة `#[allow(dead_code)]` عمّا صار مستعملاً | جزئي | زالت عن `config` و`datasource` و`devices`؛ بقيت على `AppState.identity` لأنه لم يُستعمل (انظر §5) |
| `db.rs`: `kind`/`ref` بـ`PRAGMA table_info` فقط إن غابا | تم | `has_column` عبر `pragma_table_info(?)` بـbind، والمهاجرة idempotent (اختبار يعيد تشغيلها) |
| `template_dedupe` + `claim_dedupe` ذرّية + `release_dedupe` | تم | `INSERT ... ON CONFLICT ... WHERE expires_at <= now` + `rows_affected() > 0`؛ المفتاح يُخزَّن SHA-256 hex فقط |
| `log_notification_ext` و`log_notification` تفوّض بـ`kind="raw"` | تم | التوقيع القديم بلا تغيير |
| `touch_template_used` | تم | `last_used_at` فقط، بلا لمس `version`/`definition`/`updated_at` |
| `TemplateRateLimiter` | تم | نافذة منزلقة 3600 ثانية لكل `(app, template, uid)`، سقف 10k مفتاح، `std::sync::Mutex` بلا قفل عبر `await` |
| `POST /api/v1/send/template` بالترتيب 12 خطوة | تم | الترتيب في الكود معلَّق بالأرقام 1..13: app ⇒ IP ⇒ مفتاح خدمة أو هوية+دور ⇒ قالب مفعّل ⇒ senders ⇒ حد/uid ⇒ params ⇒ doc ⇒ render ⇒ validation ⇒ dedupe ⇒ أجهزة ⇒ إرسال ⇒ سجل ⇒ رد |
| `auth.rs` إضافة فقط: `resolve_sender_role` | تم | يغلّف `policy_for` + `resolve_role`، ويحوّل أخطاءه كما كانت |
| 27 اختبار HTTP + 7 لـ`db.rs` + 7 لـ`send.rs` + 4 توصيف | تم | 45 اختباراً جديداً، بلا `ignore` |
| تحديث الوثائق والـREADME | تم | T7=✅، T8=🔄، سجل التقدم، §3.8 أضيق، §3.15 سطر 502، وإضافة `T8.md` |

## 2. الملفات (`git diff --stat 21c9072..HEAD`)

```
 src/admin/mod.rs |    5 +
 src/auth.rs      |   13 +
 src/db.rs        |  440 ++++++++++++-
 src/devices.rs   |    5 +
 src/main.rs      | 1915 ++++++++++++++++++++++++++++++++++++++++++++++++++----
 src/send.rs      |  574 ++++++++++++++++
 6 files changed, 2838 insertions(+), 114 deletions(-)
```

- `src/send.rs` (جديد): النقل الكامل لنداء FCM + `test_support::FakeTransport` + 7 اختبارات.
- `docs/plans/reports/T8-report.md` (جديد، هذا الملف).
- `docs/plans/execution/T8.md`: كان غير متتبعاً في الشجرة، أُضيف في commit الوثائق.
- كل التعديلات الأخرى ضمن ملفات القسم 0؛ `auth.rs` إضافة صرفة (13 سطراً، 0 محذوف)، و`src/db.rs` إضافة صرفة عدا سطر `log_notification` الذي صار يفوّض.

## 3. الأوامر التي شغلتها ونتائجها

| الأمر | النتيجة |
|---|---|
| `cargo test` (قبل التغييرات) | 285 ناجح، 0 فاشل |
| `cargo test` (بعد T8-1) | 289 ناجح (285 + 4 توصيف) |
| `cargo test` (بعد T8-2) | 296 ناجح (+7 `send`) |
| `cargo test` (بعد T8-3) | 303 ناجح (+7 `db`) |
| `cargo test` (بعد T8-4/5) | **330 ناجح، 0 فاشل، 0 متجاهل** (54.17s) |
| `cargo test -- --list` | `330 tests, 0 benchmarks` |
| `cargo build` / `cargo build --tests` | نجاح |
| `cargo clippy --all-targets` | `Finished` بلا أي تحذير (0 warnings) |
| `rustfmt --edition 2021 --config skip_children=true` على الملفات المعدّلة | طُبِّق |
| فحص العربية في `src/` | العربية فقط في بيانات اختبار داخل `#[cfg(test)]`: `src/main.rs:1857-1858` و`1884-1885` |
| فحص `unwrap()`/`expect(` خارج الاختبارات | لا شيء قبل `#[cfg(test)]` (سطر 1381) في `main.rs`؛ ولا في `send.rs`/`db.rs`/`auth.rs` |

الـ commits الفعلية:

```
07d9cab feat(send): T8-4 add POST /send/template
e3afebc feat(db): T8-3 add notification kind/ref, dedupe table and template usage touch
e4888a2 refactor(send): T8-2 move the FCM call behind FcmTransport
1956267 test(send): T8-1 characterization tests for the raw send route
```

(كل commit يحمل سطر `Refs: docs/plans/secure-sending-plan.md T8`، ولم يُدفع شيء.)

### التجارب المؤقتة الأربع (أُعيدت ثم أُرجعت النسخة الأصلية)

| التجربة | ما فعلته | النتيجة الفعلية |
|---|---|---|
| (أ) نقل فحص `senders` بعد `validate_params` | حذفت فحص الدور من موضعه وأعدت إدراجه بعد `validate_params` | فشل `template_role_not_in_senders_is_403`: `left: 400, right: 403` (رسالة المعامل الغائب سبقت 403) |
| (ب) `claim_dedupe` غير ذرّية (SELECT ثم INSERT) | استبدلت `ON CONFLICT` بـSELECT ثم INSERT | فشل `claim_dedupe_is_atomic_and_expires`: `UNIQUE constraint failed: template_dedupe.app, template_dedupe.template, template_dedupe.key_hash` (20 مطالبة متزامنة، نجحت أكثر من واحدة) |
| (ج) كل `INVALID_ARGUMENT` = جهاز ميت | أضفت `return true` قبل فحص `fieldViolations` | فشل `invalid_argument_on_payload_is_not_dead` |
| (د) إزالة فحص `enabled` | حذفت `.filter(|d| d.enabled)` | فشل `template_unknown_or_disabled_is_404_same_message`: `left: 200, right: 404` |

لم أحتج لتقوية اختبار الذرّية: 20 مهمة متزامنة كافية لكشف SELECT-then-INSERT على اتصال واحد.

## 4. طريقة الفحص (معايير القبول الخمسة)

| المعيار | النتيجة وكيف تحققت |
|---|---|
| 1. `git diff <T7-head>.. --stat` ضمن القسم 0، والملفات الممنوعة بلا تغيير، و`auth.rs` إضافة فقط | ✅ الملفات: `admin/mod.rs`, `auth.rs`, `db.rs`, `devices.rs`, `main.rs`, `send.rs`, و`send.rs` ملف جديد مسموح. `git diff src/auth.rs` = +13/−0. الملفات الممنوعة (`identity`, `datasource`, `config`, `template.rs`, `roles.rs`, `validation.rs`, `topics.rs`, `credentials.rs`, `Cargo.toml`, `dashboard/`) بلا أي تغيير — الاستثناء الوحيد `devices.rs` (سطران في `test_state()`، §5) |
| 2. اختبارات التوصيف تمر دون تعديل بعد commit 2 | ✅ الأربعة موجودة بنفس الأجسام من commit 1، ومرّت بعد T8-2 وبعد T8-4؛ لم أعدّل أي سطر منها |
| 3. لا `unwrap/expect` خارج الاختبارات في `send.rs`، لا `std::env`، لا تسجيل توكنات، كل SQL بـbind | ✅ `grep` على `send.rs` (كلها `unwrap_or_else`/`unwrap_or`)، لا `std::env` فيه، لا `tracing` للتوكن (التوكن لا يظهر في أي رسالة أو سجل)، وكل استعلامات `db.rs` الجديدة بمعاملات `?` مربوطة |
| 4. لا مكان في المسار يقبل `API_KEY` القديم، والترتيب يطابق القائمة | ✅ `legacy_key_cannot_send_template` = 401؛ المفتاح القديم ليس له فرع في `send_template` (فقط `verify_service_key` أو `authenticate_identity`). الترتيب مقروء في الكود بتعليقات 1..13 ويطابق القائمة |
| 5. كل أسماء الاختبارات موجودة بلا ignore، والتجارب الأربع موثقة | ✅ `cargo test -- --list` يعرض 330 اسماً، منها 45 جديدة بالأسماء المطلوبة حرفياً؛ التجارب في §3 |

تحقق إضافي من حرفية `/send`: الاختبارات الأربعة تغطي شكل النجاح، وتمرير 4xx، وفشل التوكن (502 `Could not authenticate with Firebase`)، وفشل الشبكة (502 `Network error while calling Firebase`)، وكلها تمر قبل النقل وبعده.

## 5. انحرافات عن الأمر (وسببها)

1. **`src/devices.rs`: سطران داخل `test_state()`** — `devices.rs` في قائمة الممنوع، لكن إضافة `fcm` إلى `AppState` في T8-2 جعلت `test_state()` هناك لا يُجمَّع بلا الحقلين الجديدين. أضفت فقط:
   `role_directory: Arc::new(crate::devices::RoleDirectory::new()),`
   `template_rate: Arc::new(crate::db::TemplateRateLimiter::new()),`
   **أطلب اعتماد هذا التعديل** (مثل سابقة `admin/mod.rs` في T5)؛ بدونه لا يمكن بناء اختبارات T6.
2. **`AppState.identity` بقي عليه `#[allow(dead_code)]`** — الخطوة قالت تزول عن `devices`/`identity`/`datasource`/`config` «التي صارت مستعملة». `identity` لم يُستعمل فعلياً (`AuthService` يملك نسخته)، فإزالة السماح تُنتج تحذيراً. أبقيتُه مع تعليق يوضح السبب؛ التعديل الحقيقي (حذف الحقل أو استعماله) خارج نطاق T8.
3. **`Missing Firebase access token`** — رسالة غير قابلة للوصول عملياً (تأتي من `credentials.rs` قبل بناء الرسالة)؛ كانت تُمرَّر كما هي مع 502، والآن تُترجم إلى `Could not authenticate with Firebase` مع 502 لأنها من تصنيف `Auth`. لا تغيير في الرمز.
4. **حفظ topic في `/send`** — صار بعد `FcmOutcome::Accepted` فقط (الخطوة: «حفظ topic عند النجاح»)، بينما كان سابقاً قبل نداء FCM. هذا هو التغيير الوحيد في الآثار الجانبية لـ`/send`، ولا يغيّر الرموز ولا الرسائل ولا شكل الرد.
5. **المفتاح الخدمي بلا حدّ `/uid`** — حد `template_rate` مفتاحه `(app, template, uid)`؛ طلبات المفتاح الخدمي تُنفَّذ بهوية `service` لكنها تُستثنى من الحد ومن فحص `senders` (المفتاح الخدمي ليس له دور). الخطوة لم تحدد سلوكه؛ هذا القرار يحتاج تأكيداً.
6. **عنوان commit T8-3** طوله 79 حرفاً، أعلى من حد 72 في RULES §3؛ استخدمت نص الخطوة حرفياً. أعدتُ كتابة commitي T8-3/T8-4 محلياً بالرسائل المنصوصة (المحتوى نفسه، بلا push) بعد أن كتبتهما بصياغة مختلفة.
7. **`pragma_table_info(?)`** — استخدمت الدالة الجدولية بـbind بدل `PRAGMA table_info` النصي، التزاماً بقاعدة «كل SQL بـbind»؛ النتيجة نفسها.
8. **`text` param في قالب admin-only** — اختبار `template_role_not_in_senders_is_403` يستعمل `{p.note}` نصياً في العنوان؛ مسموح لأن حاجز G1 يخص القوالب غير الإدارية ذات أهداف البث، والهدف هنا `uid`.

## 6. ملاحظات خارج النطاق (لم أنفّذها)

- `AppState.identity` حقل ميت فعلياً؛ `AuthService` يملك نسخته. حذفه أو استعماله يحتاج خطوة تنظيف.
- `ConfigStore::save_template` يرفع `version`، و`tokens_for_role`/`RoleDirectory` يستعملان `snapshot.version`؛ أي تعديل قالب يُبطل مخبأ الأدوار لكل التطبيق. قد يكون مقصوداً (أمان) لكنه مكلف.
- `validate_template` يسمح بـ`{var}` في `context.doc`/`require`/`image`/`data`/`to`/`dedupe.key` بينما المحرك يقيّد بعضها (خصوصاً `require`)، وقد يقبل قالباً سيفشل عند الإرسال (400 لكل طلب). يحتاج توحيداً.
- `validate_template` يترجم `pattern` بلا `size_limit`/anchoring رغم أن `template.rs` يفعل ذلك؛ نمط ضخم قد يستهلك ذاكرة عند الحفظ.
- `validate_data_source` يقبل `http://localhost.evil.com` (فحص البادئة النصية لا المضيف)؛ خطر SSRF محدود لكنه قائم.
- `api_fallback` لا يزال ينقصه مسارا `/devices/register` و`/devices/unregister` (موروث من T6).
- `PUBLISHED_COMMIT` في `docs/plans/reports/S0-report.md` ما زال `<يملؤه المالك>`.
- أسئلة مفتوحة من T6/T7 (تغطية ES256، رفض `alg=none`، شكل سجل `secret_env`، هل `reauth` يمدد `expires_at`) ما زالت بلا جواب.
- `taqati`: تدوير `API_KEY` القديم (R5) لم يُلمس.

# تقرير T4

- التاريخ/الوقت: 2026-10-21 13:45 +03
- الفرع و HEAD قبل/بعد: `feature/policy-auth` — قبل: `da9a6bc` — بعد الكود: `4e36dc8` (يليه commit الوثائق الذي يحمل هذا الملف)
- النطاق المنفَّذ: الجزء 1 (الواجهة + التحقق + Firestore) ثم الجزء 2 (resolver HTTP + المثال) ثم الجزء 3 (السجل) ثم التوثيق. 35 اختباراً جديداً، المجموع 187.

## 1. ما نُفِّذ (بند بند من ملف الخطوة)

| البند | الحالة | ملاحظة |
|---|---|---|
| §0 الملفات الجديدة الأربعة + `examples/` | تم | `src/datasource/{mod,firestore,http,registry}.rs`، `examples/resolver_fastapi.py`، `examples/README.md` |
| §0 `src/main.rs` سطر `mod datasource;` فقط | تم | `git diff da9a6bc..4e36dc8 -- src/main.rs` = ‎+1 سطر |
| §0 `Cargo.toml` يزيد `hmac` وحده | تم | ‎+1 سطر: `hmac = "0.12"` (لا اعتماديات أخرى) |
| §0 عدم لمس `auth.rs`/`admin/**`/`identity/**`/`topics.rs`/`validation.rs`/`credentials.rs`/`dashboard/` | تم | `git diff --stat` لهذه المسارات بين `da9a6bc` و`4e36dc8` = فارغ. `EnvSource` استُورد من `crate::identity::jwt` بلا تعديله |
| §1 `Doc` + `DataError` + `DataSource` بالتواقيع المحددة | تم | `mod.rs`؛ `async_trait`، `Send + Sync`، تعليقات إنجليزية |
| §1 `validate_doc_path` (زوجي 2..6، رفض `.`/`..`/الفراغ/التحكم، ≤ 200 بايت، بلا تقييد حروف) | تم | يعيد `Vec<&str>`؛ `auth0|123` مقبول؛ `/` لا يظهر داخل مقطع لأن التقسيم عليه |
| §1 `validate_collection` / `validate_field` (حتى 5 مقاطع نقطية) / `value` / `limit` | تم | كلها تعيد `InvalidPath` عند الخطأ |
| §1 `percent_encode` بحروف hex كبيرة | تم | `A-Za-z0-9_.~-` فقط تبقى؛ `store_orders/abc123` ⇒ `store_orders%2Fabc123` |
| §2 `AccessTokenProvider` + تنفيذ `AuthMap` بنطاق datastore | تم | `AuthMapTokenProvider` يقرأ الخريطة وقت الطلب (مثل `FirestoreHttp`)، ويحوّل فشل التوكن إلى `Unavailable` |
| §2 `FirestoreDataSource::new` + `with_base_url` | تم | الافتراضي `https://firestore.googleapis.com` |
| §2 `get_doc`: 404⇒None، 401/403 برسالة الإرشاد، غير ذلك `Firestore returned HTTP <code>`، مهلة 10ث، جسم ≤ 1MB | تم | الرسالة تحتوي `roles/datastore.viewer`؛ الجسم يُقرأ بـ`bytes()` ثم يُفحص طوله |
| §2 `members` بـ`runQuery` و`__name__` واستخراج آخر مقطع | تم | العناصر بلا `document` تُهمل؛ القيمة `stringValue` فقط (مقيَّد حالياً، موثّق في تعليق) |
| §2 `from_firestore_document` لكل الأنواع + عمق 8 + نوع مجهول | تم | **انحراف:** يعيد `Result<Doc, DataError>` لا `Doc` (السبب في §5) |
| §3 عقد `http`: `/doc` و`/members` مع `collection` | تم | الترتيب ثابت: `collection,field,value,limit` |
| §3 توقيع HMAC-SHA256 بلا Bearer + الترميز + التوقيع كما أُرسل | تم | `X-FCM-Timestamp` و`X-FCM-Signature` (hex صغير)؛ `canonical` من المسار/الاستعلام الفعليين |
| §3 متجه الاختبار في Rust والمثال والـ README | تم | القيمة مثبتة حرفياً في المواضع الثلاثة: `f97974c3…20813` |
| §3 السر من `EnvSource`، ≥ 32 بايت، رسالة موحدة، تسجيل الاسم فقط | تم | `data source secret is not configured`؛ السجل `secret_env = <name>` بلا قيمة |
| §3 رفض `base_url` غير https إلا localhost/127.0.0.1 وقت التشغيل | تم | مع فحص حدود المضيف (`localhost.evil.com` مرفوض) |
| §3 المهلة `timeout_ms`، الـ redirect، جسم ≤ 1MB | تم | مهلة لكل طلب؛ العميل يُبنى بـ`Policy::none()` في السجل والاختبارات، والمصدر يرفض أيضاً أي رد تغيّر مساره |
| §3 ردود 200/404/غير ذلك + تحقق `uids` + الاقتطاع مع warn | تم | `uids` نصية 1..128 بلا تحكم؛ أكثر من `limit` ⇒ اقتطاع + `tracing::warn!` |
| §4 مثال FastAPI + README + `--selftest` | تم | `python3 examples/resolver_fastapi.py --selftest` ⇒ `OK` (مخرج حقيقي في §3) |
| §5 السجل: Firestore/HTTP/None، مخبأ لكل `(تطبيق، إصدار)`، افتراضي Firestore مع warn مرة، مجهول ⇒ `unknown app` | تم | `DataSourceRegistry`؛ يزيل الإصدارات القديمة من المخبأ؛ **لم يُضف إلى `AppState`** |
| §6 أسماء الاختبارات المطلوبة (34) | تم | 34 اسماً مطلوباً موجودة كلها + اختبار إضافي واحد (`firestore_value_conversion_of_an_empty_document`) |
| §6 خادم HTTP/1.1 مصغّر بـ`tokio::net::TcpListener` بلا اعتماديات جديدة | تم | `test_support` داخل `mod.rs` (cfg(test)): يسجّل الطلب الخام ويرد بردود متتابعة/متأخرة/صامتة |
| §7 `fmt`/`build`/`clippy` صفر تحذير/`cargo test` كامل | تم | 187 ناجح / 0 فاشل / 0 متجاهل؛ clippy نظيف |
| §7 تجربة سلبية: عكس ترتيب معاملين في `canonical` | تم | فشل `signature_matches_known_test_vector` كما هو مطلوب، ثم استُعيد الملف (sha256 مطابق) |
| §8 أربعة commits بالأسماء المحددة | تم | `2c7e80c`، `4fb48e1`، `4e36dc8`، ثم commit التوثيق |

## 2. الملفات (git diff --stat)

```text
 Cargo.toml                   |   1 +
 examples/README.md           |  68 +++++
 examples/resolver_fastapi.py | 129 ++++++++++
 src/datasource/firestore.rs  | 582 +++++++++++++++++++++++++++++++++++++++++++
 src/datasource/http.rs       | 561 +++++++++++++++++++++++++++++++++++++++++
 src/datasource/mod.rs        | 451 +++++++++++++++++++++++++++++++++++++++++
 src/datasource/registry.rs   | 423 +++++++++++++++++++++++++++++++
 src/main.rs                  |   1 +
 8 files changed, 2216 insertions(+)
```

- `src/datasource/mod.rs` (جديد): `Doc`، `DataError`، `DataSource`، دوال التحقق والترميز، وخادم الاختبار المصغّر.
- `src/datasource/firestore.rs` (جديد): مزود التوكن، `get_doc`، `members`، `from_firestore_document`، 11 اختباراً.
- `src/datasource/http.rs` (جديد): العقد الموقّع، التوقيع، حدود الردود، 12 اختباراً.
- `src/datasource/registry.rs` (جديد): اختيار المصدر لكل `(تطبيق، إصدار)`، مصدر `none`، 7 اختبارات.
- `examples/resolver_fastapi.py` (جديد): مثال الباك إند + `--selftest` بمكتبة Python القياسية.
- `examples/README.md` (جديد): العقد، متجه الاختبار، التشغيل، تنبيه الـ reverse proxy (بالإنجليزية كما يشترط §1).
- `Cargo.toml`: سطر `hmac` فقط. `src/main.rs`: سطر `mod datasource;` فقط.
- في commit التوثيق: `docs/plans/secure-sending-plan.md` (§3.4 + T13 + سجل التقدم)، `docs/plans/execution/README.md`، `docs/plans/execution/T4.md`، وهذا التقرير.

## 3. الأوامر التي شغلتها ونتائجها (مخرجات مختصرة حقيقية)

| الأمر | النتيجة |
|---|---|
| `cargo build` | `Finished dev profile … in 2.08s` — بلا تحذير |
| `cargo clippy --all-targets` | `Finished dev profile … in 1.23s` — صفر تحذير (بعد إصلاح `manual_is_multiple_of` الوحيد الذي ظهر) |
| `cargo test` | `test result: ok. 187 passed; 0 failed; 0 ignored` |
| `cargo test -- --list \| grep datasource` | 35 اختباراً (34 مطلوباً + 1 إضافي) |
| `python3 examples/resolver_fastapi.py --selftest` | `OK` (exit=0) |
| فحص عربي على `src`/`examples` | صفر نتيجة (أسماء/تعليقات إنجليزية فقط) |
| `grep -rn "std::env::var" src/datasource` | لا شيء |
| `grep -rn "unwrap()\|\.expect(" src/datasource` | كل النتائج داخل `#[cfg(test)]`/`test_support` فقط |
| فحص إضافي لِمُتحقِّق المثال (stdlib) | `valid=True`, `bad_signature=False`, `stale_timestamp=False`, `changed_query=False`, `missing_header=False` |
| التجربة السلبية | بعد عكس `path`/`query` في الإنتاج: `assertion left == right failed … failures: signature_matches_known_test_vector … FAILED 0 passed; 1 failed` ثم الاستعادة |
| sha256 لـ`http.rs` قبل/بعد التجربة | `45ca5d5060da8819f568f0f9f97c7a2fdb9e6bdc1cb5db5f57d74905fad090b7` في الحالتين |

## 4. طريقة الفحص: كل بند من "طريقة الفحص" في الخطة/الخطوة ← النتيجة

| البند | النتيجة |
|---|---|
| اختبارات تحويل قيم Firestore | `firestore_value_conversion_all_types` (نص، i64، i64 غير صالح كنص، double، bool، null، timestamp، reference، bytes، geoPoint، map، array، خريطة/مصفوفة فارغة) + نوع مجهول + عمق 8 مسموح/9 مرفوض + وثيقة بلا `fields` ⇒ `{}` |
| خادم HTTP وهمي: 200 ⇒ وثيقة | `http_get_doc_signs_request_and_parses_object` و`firestore_get_doc_found` (الرد يُفكّ لكائن) |
| 404 ⇒ `None` | `http_get_doc_404_is_none` و`firestore_get_doc_not_found_is_none`، و`http_members_404_is_unavailable` للـ members |
| 500/مهلة ⇒ `Unavailable` | `firestore_get_doc_500_is_unavailable`، `http_timeout_is_unavailable`، `http_does_not_follow_redirects` (302) |
| التوقيع يُحسب كما يتوقعه المثال | متجه الاختبار نفسه في Rust/Python/README؛ `http_get_doc_signs_request_and_parses_object` يعيد حساب HMAC من الطلب الخام باستقلال عن دالة الإنتاج |
| لا يظهر السر في الطلب | `http_secret_never_appears_in_request` + غياب `Authorization` |
| تشغيل مثال FastAPI محلياً ضد الأداة (يدوي) | **لم يتم**: `fastapi`/`uvicorn` غير مثبتين في البيئة (`ModuleNotFoundError`)، ولم أثبّت حزم Python. عوضاً عنه: `--selftest` نجح، وفحص `verify()` مباشرةً (بلا FastAPI) أعطى النتائج في §3 |
| `--list` بأسماء الاختبارات | 35 اسماً تحت `datasource::` كما في §3 |
| التجربة السلبية | تمت وفشلت الاختبارات كما هو مطلوب ثم أُعيد الملف حرفياً |

## 5. انحرافات عن الأمر (وسببها)

1. **`from_firestore_document` يعيد `Result<Doc, DataError>`** بدل `Doc` كما في توقيع §2: الأمر نفسه يفرض `Unavailable` عند النوع المجهول والعمق > 8؛ إرجاع `Doc` مباشرة يعني panic أو قطعاً صامتاً، وكلاهما يخالف مبدأ fail-closed و§6 (اختبارا العمق والنوع). الاختبارات تستدعي `expect`/`expect_err` على النتيجة.
2. **اختبار إضافي** `firestore_value_conversion_of_an_empty_document` (وثيقة بلا `fields` ⇒ `{}`)، لأن Firestore يعيدها كذلك فعلاً؛ ليس مطلوباً بالاسم لكنه سلوك حدودي.
3. **`signature_matches_known_test_vector`** يجمع تأكيدين: (أ) أن `canonical_request` الإنتاجية تساوي النص القانوني حرفياً، (ب) HMAC يُحسب في الاختبار بمكتبة `hmac` مباشرة على النص الثابت (لا استدعاء `sign`). هذا يرضي شرط §6 (حساب مستقل) و§7 (فشل التجربة السلبية عند تغيير ترتيب الإنتاج) معاً؛ بلا التأكيد (أ) لن تلاحظ التجربة السلبية تغيير الإنتاج.
4. **اقتطاع `members` في Firestore** عند تجاوز `limit` مع `tracing::warn!` (الأمر نص عليه لـ HTTP فقط)؛ الطلب فيه `limit` أصلاً، والاقتطاع دفاع إضافي ليبقى العقد "at most limit".
5. **الـ redirect:** `HttpDataSource::new` يستقبل العميل حسب التوقيع المحدد، فلا يمكنه فرض `Policy::none()` بنفسه؛ السجل والاختبارات يبنيانه بـ`Policy::none()`، والمصدر يرفض أيضاً رداً تغيّر مساره (`response.url().path()`). موثّق في تعليق أعلى `new`.
6. **رسالة `members` عند 404** نصها `resolver returned 404 for a members query` (الأمر حدد `Unavailable` دون نص حرفي).
7. **حجم مثال Python** 129 سطراً لا ~80: أغلب الزيادة docstring و`selftest` وقاعدة البيانات التجريبية.
8. **رسائل commit** مأخوذة حرفياً من §8، وهي أطول من حد 72 حرفاً المذكور في RULES §3؛ التزمت بنص الخطوة.
9. **`examples/README.md` بالإنجليزية** لأن فحص العربية في §1 يغطي `examples/` كلها.

## 6. ملاحظات خارج النطاق

- **تحقق الإعداد أضعف من تحقق وقت التشغيل:** `validate_data_source` في `config/validate.rs` يقبل `http://localhost.evil.com` (فحص `starts_with`)، بينما `HttpDataSource` يرفضه بحدود مضيف صحيحة. الملف ممنوع في §0 فلم يُعدَّل.
- **`test_state()` في `src/admin/mod.rs`:** لم يُضَف `datasource` إلى `AppState` في هذه الخطوة كما طُلب؛ سيتطلب تعديل سطر واحد في `test_state()` عند الربط في T5 (قرار README).
- **تكرار منطق التوكن:** `AuthMapTokenProvider` يكرر جزءاً من `FirestoreHttp::read_user` في `auth.rs`؛ `auth.rs` ممنوع هنا، والتوحيد مرشح لـ T5.
- **`FirestoreHttp`** و`AuthService` لم يُلمسا كما طُلب (§6 من الخطوة).
- **التحقق من `timeout_ms`** في وقت التشغيل غير موجود داخل المصدر (التحقق في الإعداد يضمن 200..10000)؛ لم أُضف تحققاً غير مطلوب.
- **ذاكرة الجسم:** سقف 1MB يُفحص بعد قراءة الجسم كاملاً؛ لا سقف أثناء القراءة (reqwest لا يوفره مباشرة هنا). ذُكر كسؤال في §7.

## 7. أسئلة للمالك/المراجع

1. هل يُعتمد توقيع `from_firestore_document -> Result<Doc, DataError>` (انحراف 1)؟
2. هل يُقبل أن يبقى فرض `Policy::none()` على العميل الممرَّر (السجل/الاختبارات) مع فحص المسار النهائي في المصدر، أم تريد أن يبني المصدر عميله بنفسه في إصلاح؟
3. هل اقتطاع `members` في Firestore مع warn مقبول (انحراف 4)؟
4. هل سقف 1MB بعد القراءة كافٍ، أم تريد فحص `Content-Length` قبل القراءة أيضاً (يتطلب قراءة متدفقة بسقف)؟

## 8. حالة git النهائية

```text
$ git status --short
 M docs/plans/execution/README.md
 M docs/plans/secure-sending-plan.md
?? docs/plans/execution/T4.md

$ git log --oneline -4
4e36dc8 feat(datasource): T4-3 add data source registry
4fb48e1 feat(datasource): T4-2 add signed HTTP resolver source and FastAPI example
2c7e80c feat(datasource): T4-1 add data source interface, path validation and Firestore source
da9a6bc docs(plan): T3 update execution tracker
```

(الحالة أعلاه قبل commit التوثيق الذي يضم هذه الملفات الأربعة؛ لا push.)

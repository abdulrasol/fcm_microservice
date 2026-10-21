# تقرير T7
- التاريخ/الوقت: 2026-10-21 16:31 +03
- الفرع و HEAD قبل/بعد: `feature/policy-auth`، قبل `55faf5b`، بعد commits الكود `e3ad3a4` (ثم commit التوثيق هذا).

## 1. ما نُفِّذ (بند بند من ملف الخطوة)

| البند | الحالة | ملاحظة |
|---|---|---|
| §0 التعديلات المسموحة | تم | جديد `src/template.rs` + سطر `mod template;` في `main.rs`، وإضافة صرفة في `src/config/placeholder.rs` (116 سطراً مضافاً، **0 محذوف**)، و`docs/plans/**`. لا ملف آخر تغيّر (`git diff --numstat 55faf5b..e3ad3a4` في §2). |
| §0 `Segment` + `segments()` | تم | `pub enum Segment { Text(String), Ref(Placeholder) }` و`pub fn segments(s:&str) -> Result<Vec<Segment>, String>`. التحليل **موكول** إلى `extract` (نفس القواعد ونفس الرسائل حرفياً) و`segments` يحدّد المواقع فقط عبر `find_close` الموجود؛ لا تكرار لمنطق التحليل، ولا تعديل لأي دالة أو اختبار موجود. الاختباران `segments_preserves_order_and_literals` و`segments_same_errors_as_extract` (يقارن رسائل 9 مدخلات خاطئة مع `extract`). |
| §1 `Sender`/`ValidatedParams`/`RenderError`/`Rendered`/`ResolvedTarget` | تم | الحقول والتواقيع كما في الأمر حرفياً (`validate_params`، `context_path`، `render`). `RenderError` يحمل `BadRequest(String)` أو `Forbidden` فقط. |
| §1 `render` يفحص `require` أولاً | تم | `check_require` يُنادى قبل تصيير `title`/`body`/`image`/`data`/`to`/`dedupe` (اختبار `require_runs_before_other_substitution`). |
| §2 المعاملات: كائن/null/غير كائن | تم | غير الكائن ⇒ `BadRequest`؛ `null` مقبول فقط إذا لم تُعرَّف معاملات؛ مفتاح غير معرَّف ⇒ 400؛ معامل معرَّف غائب ⇒ 400. |
| §2 `text` | تم | نص فقط، `chars().count() <= max`، رفض الفارغ تماماً، ورفض أحرف التحكم عدا `\n` و`\t`، **بلا قصّ أو تعديل** للقيمة. |
| §2 `pattern` | تم | `^(?:{pattern})$` عبر `RegexBuilder::size_limit(100_000)`، رفض pattern > 200 بايت، `max` بالأحرف إن وُجد، ورفض أي حرف تحكم دائماً. |
| §2 `enum` | تم | مطابقة حرفية تامة (لا تطبيع ولا حالة). |
| §2 `int` | تم | عدد JSON صحيح أو نص صحيح، `min`/`max`، العشري (JSON أو نص) ⇒ 400، ويُخزَّن نصاً عشرياً قانونياً (`7` من `7` و`"7"`). |
| §2 حد 4096 بايت | تم | يُقاس قبل أي معالجة (طول النص، أو تمثيل الرقم). |
| §2 التعويض بمرور واحد | تم | `render_text` يمرّ على `segments()` مرة واحدة ويُلحق النص الحرفي والقيم؛ القيمة لا تُفحص كقالب ثانية. |
| §2 المصادر | تم | `p.*` من `ValidatedParams`، `sender.uid|name|role` (الغائب ⇒ 400)، `doc.<path>`/`entity.<path>` بمفتاح كائن فقط (`Value::get` بمقطع نصي؛ لا فهارس مصفوفات)، و`{var}` من `vars`. |
| §2 التحويل إلى نص | تم | string كما هي، عدد صحيح عشري قانوني، عشري بأقصر تمثيل (`150000.0` ⇒ `150000`، `1.25` ⇒ `1.25`)، bool ⇒ `true|false`، و`null`/مفقود/كائن/مصفوفة ⇒ 400. |
| §2 فلتر `|int` | تم | `round` بعيداً عن الصفر، سقف `|x| <= 1e15`، غير الرقمي/البوليني ⇒ 400؛ `150000.0|int` ⇒ `150000`، `"150000.6"|int` ⇒ `150001`، `2.5` ⇒ `3`، `-2.5` ⇒ `-3`، و`-0` يُطبَّع إلى `0`. |
| §2 أحرف التحكم في القيم المعوَّضة | تم | رفض كل أحرف التحكم عدا `\n` و`\t` بعد التعويض. |
| §2 مقاطع المسار | تم | في `context_path` وحدها: رفض `/` و`\` و`..` وأحرف التحكم والفارغ في كل قيمة معوَّضة؛ النص الحرفي للقالب لا يُفحص. |
| §2 `vars` | تم | `from` بلا أقواس (`doc./p./sender./entity.`)، قراءة القيمة كنص ثم مطابقة تامة في `map`، الغياب ⇒ `BadRequest("unsupported value")` **بلا ذكر القيمة**، قيمة `map` تُصيَّر كقالب عادي مع منع أي `{var}` داخلها (دفاع)، والحساب كسول مرة واحدة عند أول استعمال ويُخزَّن. |
| §2 `context.require` | تم | لكل (حقل، قالب): القيمة من p/sender فقط تُقارَن نصاً بقيمة الحقل في `doc` بنفس قواعد التحويل وبلا فلتر. `doc` غائب أو الحقل غائب أو عدم التطابق ⇒ `RenderError::Forbidden` نفسه في الحالات الثلاث بلا نص مميّز. |
| §2 حدود الناتج | تم | `title`/`body` بالأحرف، `image` بالبايت، `data` بحجم JSON المتسلسل، كلها من ثوابت `validation.rs` (تفصيل في §5). `title` فارغ بعد التعويض ⇒ 400. |
| §2 مفاتيح `data` المحجوزة | تم | `from`، `notification`، `message_type`، `google.*`، `gcm.*` ⇒ 400 (دفاع ثانٍ عبر `config::validate::is_reserved_data_key`). |
| §2 الهدف | تم | `uid` عبر `identity::is_valid_uid`، `role` عبر `config::validate::is_role` (`^[a-z0-9_]{1,32}$`)، و`topic`/`condition` تُرجَع كما هي بلا تحقق إضافي (لـT8). |
| §2 `dedupe` | تم | `dedupe.key` يُعوَّض بنفس القواعد ويُرجَع مع `seconds`؛ لا منطق dedupe هنا. |
| §2 العربية واليونيكود | تم | لا تطبيع ولا تحويل حالة؛ اختبار `arabic_text_is_preserved_exactly` يقارن السلاسل حرفياً. |
| §2 بلا panic | تم | لا `unwrap`/`expect`/`std::env`/فهرسة خارج `#[cfg(test)]` (فحص آلي في §3)؛ لا اقتطاع نصي إطلاقاً. |
| §3 الاختبارات بالأسماء المطلوبة | تم | 9 params + 17 render + 8 context/require + 5 vars + التكامل + القفل = 41 في `template.rs`، و2 في `placeholder.rs`؛ كلها موجودة بلا `#[ignore]` (القائمة في §3). |
| §3 `watt_example_templates_render` | تم | يحمّل `policies/watt.example.json` عبر `ConfigDocument` ويُصيّر `admin_alert` و`order_new` و`order_status` بوثائق وهمية؛ يتحقق من `150000.0` ⇒ `150000`، ومن `context_path` = `store_orders/abc123`، ومن `dedupe`، ومن الهدف. |
| §3 `renderer_has_no_io_dependencies` | تم | فحص ثابت عبر `include_str!("template.rs")` عن `reqwest`/`sqlx`/`tokio::fs`/`std::env`، والسلاسل مبنية بالتقطيع (`"reqw"+"est"`) كي لا يطابق الاختبار نفسه. آلي لا يدوي. |
| §4 الجودة | تم | `cargo fmt` على الملفين، `cargo build` بلا تحذير، `cargo clippy --all-targets` صفر تحذير، `cargo test` 285 ناجحاً. `#[allow(dead_code)]` على مستوى وحدة `template.rs` مع تعليق أنه حتى T8. |
| §4 التجارب الثلاث | تم | الثلاث أُجريت وفشّلت الاختبار المقصود ثم أُعيد الملف وتحقّقت `sha256` (§3). |
| §5 الأربعة commits | تم | `8c2b406`، `936d78c`، `e3ad3a4`، ثم commit التوثيق هذا؛ بلا push. |
| §5 التوثيق | تم | `execution/README.md` (T6=✅، T7=🔄)، سجل التقدم §9، `execution/T7.md`، وهذا التقرير. |
| معيار القبول 1 | تم | `git diff 55faf5b..e3ad3a4 --stat` = 3 ملفات فقط (المسموحة)؛ `placeholder.rs` 116+/0−. |
| معيار القبول 2 | تم | لا `unwrap/expect` خارج الاختبارات، ولا `reqwest|sqlx|tokio::fs`، ولا `std::env` (فحص في §3). |
| معيار القبول 3 | تم | فحص العربية: 12 سطراً كلها داخل `#[cfg(test)]` (بيانات اختبار مبرَّرة) و2 في اختبارات `placeholder.rs`؛ صفر خارجها (§3). |
| معيار القبول 4 | تم | كل الأسماء موجودة، لا `#[ignore]`، والتجارب الثلاث موثقة بمخرجاتها. |
| معيار القبول 5 | تم | `forbidden_is_uniform_for_all_three_cases` يقارن `RenderError` المُرجَع فعلياً في الحالات الثلاث بـ`assert_eq!` (لا رسائل). |

## 2. الملفات

`git diff --stat 55faf5b..e3ad3a4`:
```
 src/config/placeholder.rs |  116 ++++
 src/main.rs               |    1 +
 src/template.rs           | 1527 +++++++++++++++++++++++++++++++++++++++++++++
 3 files changed, 1644 insertions(+)
```
`git diff --numstat 55faf5b..e3ad3a4`:
```
116	0	src/config/placeholder.rs
1	0	src/main.rs
1527	0	src/template.rs
```

- `src/template.rs` (جديد، 1527 سطراً): الأنواع العامة (`Sender`، `ValidatedParams`، `RenderError`، `ResolvedTarget`، `Rendered`) + `validate_params` + `context_path` + `render` + `Renderer` الداخلي (تعويض، `vars` كسولة، `require`) + مساعدات التحويل والحدود + 41 اختباراً. لا شبكة ولا قاعدة بيانات ولا بيئة.
- `src/config/placeholder.rs` (+116/−0): `Segment` و`segments()` واختباراه فقط؛ `extract`/`is_literal`/`parse_placeholder`/`is_name`/`is_path` والاختبارات القائمة لم تُمَس.
- `src/main.rs` (+1): `mod template;` فقط.
- `docs/plans/execution/T7.md` (كان غير متتبع فأُضيف)، `docs/plans/execution/README.md`، `docs/plans/secure-sending-plan.md` (§9)، `docs/plans/reports/T7-report.md`.

## 3. الأوامر التي شغلتها ونتائجها

- `cargo build` ⇒ `Finished dev profile` بلا تحذير.
- `cargo clippy --all-targets` ⇒ `Finished` بلا أي تحذير.
- `cargo test` ⇒ `test result: ok. 285 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out` (الأساس 242، الجديد 43 = 41 في `template.rs` + 2 في `placeholder.rs`).
- `cargo test -- --list` (مفلتراً) ⇒ 43 اسماً جديداً:
  `segments_preserves_order_and_literals`، `segments_same_errors_as_extract`،
  `param_text_counts_chars_not_bytes`، `param_text_rejects_empty_and_control_chars`، `param_pattern_is_anchored_both_ends`، `param_pattern_oversized_regex_rejected`، `param_enum_exact_match_only`، `param_int_bounds_and_rejects_float`، `param_unknown_key_rejected`، `param_missing_rejected`، `param_value_over_4096_bytes_rejected`،
  `render_substitutes_all_sources`، `p_value_with_placeholder_is_not_reinterpreted`، `doc_value_with_placeholder_is_not_reinterpreted`، `int_filter_rounds_150000_point_0`، `int_filter_rounds_half_away_from_zero`، `int_filter_rejects_non_numeric_and_huge`، `float_without_filter_prints_integer_form`، `missing_or_null_or_object_field_is_bad_request`، `arabic_text_is_preserved_exactly`، `control_chars_in_substituted_value_rejected`، `title_empty_after_render_rejected`، `title_body_limits_enforced`، `data_total_size_limit_enforced`، `reserved_data_key_rejected`، `target_uid_must_be_valid_uid`، `target_role_charset_enforced`، `dedupe_key_rendered`،
  `context_path_rejects_slash_dotdot_and_backslash_in_values`، `context_path_none_without_context`، `require_match_passes`، `require_mismatch_is_forbidden`، `require_missing_doc_is_forbidden`، `require_missing_field_is_forbidden`، `forbidden_is_uniform_for_all_three_cases`، `require_runs_before_other_substitution`،
  `var_maps_doc_value`، `var_unknown_value_is_bad_request_without_echo`، `var_map_value_may_use_other_sources`، `var_referencing_var_rejected`، `var_computed_lazily_and_only_when_used`،
  `watt_example_templates_render`، `renderer_has_no_io_dependencies`.
- `rustfmt --edition 2021 --config skip_children=true --check src/main.rs src/template.rs src/config/placeholder.rs` ⇒ لا فرق (exit 0).
- فحص ما قبل `#[cfg(test)]`:
  `awk '/^#\[cfg\(test\)\]/{exit} {print NR": "$0}' src/template.rs | grep -nE "\.unwrap\(\)|\.expect\(|std::env|println!|eprintln!"` ⇒ لا شيء (`#[cfg(test)]` في السطر 638).
- فحص العربية (`perl -CSD -ne '... /[\x{0600}-\x{06FF}]/'` على `src` و`examples` باستثناء `validation.rs`/`topics.rs`): 12 سطراً في `template.rs` (705، 706، 708، 979، 983، 988، 989، 1463، 1464، 1500، 1501 — كلها بعد السطر 638) و2 في `config/placeholder.rs` (371، 374 بعد `#[cfg(test)]` في السطر 259). **صفر خارج بيانات الاختبار**. الاستثناء مبرَّر بنص §0 من ملف الخطوة (اختبار «النص العربي يُحفظ كما هو» إلزامي).
- `cargo test template` بعد كل commit ⇒ `ok` (النسخة الوسيطة للـcommit الثاني: 19 ناجحاً = 9 params + 2 context_path + 8 من اختبارات `config::store` التي يطابق اسمها `template`).
- **التجربة السلبية (أ)** (إعادة تمرير الناتج على القالب داخل `render_text`):
  أول محاولة كانت `self.render_text(&out, ...)` بلا شرط فأدّت إلى `has overflowed its stack` (تكرار لا نهائي حتى على نص بلا مراجع)؛ استبدلتها بالنسخة المقيَّدة `if out.contains('{') { return self.render_text(&out, mode, allow_vars); }` كي تقيس إعادة التفسير فعلاً. النتيجة:
  `test template::tests::p_value_with_placeholder_is_not_reinterpreted ... FAILED`
  `assertion left == right failed: left: "leaked"  right: "{doc.secret}"`،
  `test result: FAILED. 0 passed; 1 failed`.
- **التجربة السلبية (ب)** (تجاوز حالة الوثيقة الغائبة في `require`):
  `let document = match doc { Some(document) => document, None => return Ok(()) };` ⇒
  `test template::tests::require_missing_doc_is_forbidden ... FAILED`
  `expected Forbidden, got Err(BadRequest("template needs a context document"))`،
  `test result: FAILED. 0 passed; 1 failed`.
- **التجربة السلبية (ج)** (pattern غير مثبّتة: `let anchored = pattern.to_string();`) ⇒
  `test template::tests::param_pattern_is_anchored_both_ends ... FAILED`
  `expected a bad request, got Ok(ValidatedParams({"code": "abc1"}))`،
  `test result: FAILED. 0 passed; 1 failed`.
- بعد كل تجربة أُعيد `src/template.rs` من نسخة احتياطية في مجلد temp المعتمد، وتحقّقت `sha256 = 00b18871bbebb5c6c3481fe87626e16ce1a7d14df431f5adf3b43ac703a37440` قبل التجارب وبعد كل استعادة؛ ثم `cargo test` ⇒ 285 ناجحاً.

## 4. طريقة الفحص: كل بند من «طريقة الفحص» ← النتيجة

| الفحص المطلوب (الخطة T7 + ملف الخطوة) | النتيجة وكيف تحققت |
|---|---|
| قيمة `p` تحتوي `{doc.x}` لا تُفسَّر | `p_value_with_placeholder_is_not_reinterpreted`: `p.text = "{doc.secret}"` مع `doc.secret = "leaked"` ⇒ العنوان يبقى `{doc.secret}` حرفياً. وتجربة (أ) تُفشِل هذا الاختبار عند إعادة التمرير. |
| قيمة `doc` تحتوي مرجعاً لا تُفسَّر | `doc_value_with_placeholder_is_not_reinterpreted`: `doc.note = "{p.text}"` ⇒ يبقى `{p.text}`. |
| `require` غير مطابق ⇐ خطأ | `require_mismatch_is_forbidden` (403)، و`require_missing_doc_is_forbidden`، و`require_missing_field_is_forbidden`، والثلاثة متساوية في `forbidden_is_uniform_for_all_three_cases`. |
| `require` قبل أي تعويض آخر | `require_runs_before_other_substitution`: الوثيقة بلا `note` (سيُنتج 400) و`require` فاشل ⇒ الناتج `Forbidden` لا `BadRequest`. |
| `vars` بقيمة غير موجودة ⇐ خطأ | `var_unknown_value_is_bad_request_without_echo`: الرسالة تحتوي `unsupported value` ولا تحتوي القيمة `top-secret-status`. |
| `vars` كسولة ولا تُحسب إلا عند الاستعمال | `var_computed_lazily_and_only_when_used`: var غير مستعملة `from = doc.missing` لا تُفشِل التصيير. |
| `|int` على `150000.0` ⇐ `150000` | `int_filter_rounds_150000_point_0`، و`int_filter_rounds_half_away_from_zero` (2.5 ⇒ 3، −2.5 ⇒ −3، `"150000.6"` ⇒ `150001`). |
| `/` في قيمة مسار ⇐ رفض | `context_path_rejects_slash_dotdot_and_backslash_in_values`: `../other`، `a/b`، `a\b`، `a\u{0}b`، `""` كلها 400؛ `abc-123` ⇒ `store_orders/abc-123`. |
| نص عربي يُحفظ كما هو | `arabic_text_is_preserved_exactly` (عنوان ونص)، و`watt_example_templates_render` (قوالب المثال العربية تُصيَّر بنفس النص). |
| pattern مثبّتة من الطرفين | `param_pattern_is_anchored_both_ends`: `[a-z]+` ترفض `abc1` و`1abc`؛ تجربة (ج) تُفشِله. |
| pattern ضخم ⇐ رفض | `param_pattern_oversized_regex_rejected`: pattern 201 بايت، و`(a{1000}){1000}` الذي يتجاوز `size_limit`. |
| حد 4096 بايت للقيمة الخام | `param_value_over_4096_bytes_rejected`. |
| الطول بالأحرف لا البايتات | `param_text_counts_chars_not_bytes`: `مرحبا` (5 أحرف/10 بايت) مقبولة عند `max=5`، و6 أحرف مرفوضة. |
| حدود الناتج | `title_body_limits_enforced` (200/4000 حرفاً، والقيم على الحد تُقبل)، `data_total_size_limit_enforced`، و`title_empty_after_render_rejected`. |
| مفاتيح `data` المحجوزة | `reserved_data_key_rejected` لكل المفاتيح الخمسة. |
| الهدف | `target_uid_must_be_valid_uid` (فارغ و129 بايت ⇒ 400)، `target_role_charset_enforced` (`Admin`، مسافة، فارغ، 33 حرفاً ⇒ 400؛ `admin_1` ⇒ OK). |
| `dedupe.key` يُعوَّض | `dedupe_key_rendered` ⇒ `abc:shipped` مع `seconds=600`، و`None` بلا قسم dedupe. |
| مثال واط الحقيقي | `watt_example_templates_render`: `admin_alert` (role=admin، `data.target=/brands/acme`)، `order_new` (`طلب جديد #42`، `بقيمة 150000 IQD`، target uid `seller-7`، `context_path=store_orders/abc123`، dedupe `abc123`)، `order_status` (`تم شحن طلبك #42...`، target uid `user-1`، dedupe `abc123:shipped`). |
| نقاء المحرك | `renderer_has_no_io_dependencies` (فحص `include_str!` آلي)، بالإضافة إلى الفحص الآلي لغياب `std::env`/الطباعة في §3. |

## 5. انحرافات عن الأمر (وسببها)

1. **`context_path` و`vars`:** نص الأمر «renders only p./sender. refs»؛ سمحت أيضاً بـ`{var}` في `context.doc` بشرط أن يكون `from` من `p.`/`sender.` فقط (أي `doc.`/`entity.` داخل المسار ⇒ 400)، لأن `validate_template` يسمح أصلاً بـ`{var}` في `context.doc`. المراجع يقرر: أُبقي السلوك (أوسع من النص) أم أُمنع `{var}` كلياً في المسار. **لم أضف أي مصدر جديد للمسار.**
2. **`require` و`vars`:** تُحسب قيم `require` من p/sender فقط كما طلب الأمر؛ أي `{var}` داخلها ⇒ 400 لا 403. لكن `validate_template` يسمح بـ`{var}` في `require`، فقد يُحفظ قالب يفشل عند التصيير (ملاحظة خارج النطاق في §6).
3. **حد `data`:** حسبته بحجم JSON المتسلسل (`serde_json::to_vec`) تماماً كما في `validation::validate_send`، لأن الأمر أحال إلى «كيف تُحسب هناك». هذا يغطي شرط «مفاتيح+قيم ≤ 4096» ضمناً (الحجم المتسلسل ≥ مجموعهما)، ويضمن ألا يقبل T7 ما سيرفضه T8.
4. **فراغ `title`:** استعملت `title.trim().is_empty()` (مطابق لِـ`[T3]` عند الحفظ و`validate_send`)، لا `is_empty()` فقط.
5. **`image`:** فحصت الطول فقط (2048 بايت) كما في بند الحدود؛ لم أضف فحص `https://` لأنه من `validation.rs` في T8 وليس في قائمة حدود T7.
6. **تقسيم commits 2/3:** لأن `git add` بالاسم فقط، استعملت نسختين مرحليتين من `template.rs` (نسخة A: الأنواع + `validate_params` + `context_path` + 11 اختباراً؛ ثم النسخة الكاملة +30 اختباراً)، على سابقة T6. كل commit يبني واختباراته تنجح، والنسخة النهائية هي `e3ad3a4`.
7. **التجربة (أ):** المحاولة الأولى (إعادة تمرير غير مشروطة) انتهت بـstack overflow بدل فشل تأكيد؛ أعدتها بصيغة مقيَّدة بـ`out.contains('{')` كي تقيس إعادة التفسير فعلاً. الملف أُعيد وتحقّقت `sha256` في الحالتين.

## 6. ملاحظات خارج النطاق

- `config::validate::validate_template` يسمح بـ`{var}` داخل `context.doc` و`context.require` و`image`/`data`/`to`/`dedupe.key`، لكن المحرك يحسب `require` من p/sender فقط (حسب الأمر). قالب كهذا يُحفظ ثم يفشل بـ400 عند التصيير. الإصلاح المقترح (خارج T7): منع `{var}` في `require` عند الحفظ، أو السماح بها في المحرك — قرار المراجع.
- `validate_template` يبني `regex::Regex::new(pattern)` **بلا** `size_limit` وبلا تثبيت عند الحفظ (يكفي أنه يُترجم)، بينما المحرك يثبّت ويحدّ الحجم. قالب بـpattern ضخم قد يُستهلك عند الحفظ (مسار إداري محمي) ثم يُرفض عند التصيير. لم أعدّل `validate.rs`.
- `validate_template` يسمح بـ`text.max` حتى 1000 حرف، والمحرك يرفض أي قيمة خام > 4096 بايت؛ 1000 حرف عربي = 2000 بايت، فلا تعارض عملي، لكن الحدّين مستقلان.
- `admin_alert` في `policies/watt.example.json` هدفه `role: "admin"` بلا `context`؛ لا علاقة له بـ`require`. لم أعدّل المثال.
- لا تغيير على `validate.rs`، `Cargo.toml`، `AppState`، ولا أي مسار HTTP (كما طُلب).

## 7. أسئلة للمالك/المراجع

1. `context_path`: هل يُسمح بـ`{var}` (من `p.`/`sender.` فقط) كما نفّذت، أم يُمنع كلياً كما في النص الحرفي؟
2. `require` مع `{var}`: أُبقي 400 (كما نفّذت) أم يُمنع عند الحفظ في `validate.rs` (إصلاح لاحق)؟
3. حد `data`: هل الحجم المتسلسل JSON مقبول كتفسير لـ«مفاتيح+قيم» (وهو ما يضمن اتفاق T7 مع T8)؟
4. هل تريد توحيد `MAX_PATTERN_BYTES`/`REGEX_SIZE_LIMIT` في `validation.rs` بدل تعريفها في `template.rs` (خارج نطاق T7)؟
5. مواضع النص العربي في بيانات الاختبار (12 سطراً في `template.rs` + 2 في `placeholder.rs`) — مذكورة في §3، هل الاستثناء مقبول بهذا الشكل؟

## 8. حالة git النهائية

قبل commit التوثيق (`e3ad3a4`):
```
$ git status --short
?? docs/plans/execution/T7.md
$ git log --oneline -5
e3ad3a4 feat(template): T7-3 add renderer with vars, require and limits
936d78c feat(template): T7-2 add param validation and context path
8c2b406 feat(config): T7-1 add placeholder segments parser
55faf5b docs(plan): T6 update execution tracker
51126d3 feat(devices): T6-3 add register and unregister endpoints
```
بعد commit التوثيق هذا يصبح الشجرة نظيفة والفرع `feature/policy-auth` بلا push.

# تقرير T1

PUBLISHED_COMMIT: <يملؤه المالك>

- **التاريخ/الوقت:** 2026-10-21
- **الفرع و HEAD قبل/بعد (`git rev-parse --short HEAD`):**
  - قبل: `feature/policy-auth` @ `8a08046` (نهاية S0)
  - بعد: `feature/policy-auth` @ **010a879 + <hash2>** (commitان: كود ثم توثيق — انظر القسم 8)
- **المستودع:** `fcm_microservice` فقط. **لم يُمس `taqati`** (لا علاقة لـT1 به).

---

## 1. ما نُفِّذ (بند بند من ملف الخطوة)

| البند | الحالة | ملاحظة |
|---|---|---|
| **0.** التعديلات المسموحة فقط | تم | جديد: `src/config/{mod,placeholder,validate,store}.rs`، `src/cli.rs`، `policies/watt.example.json`. تعديل: `src/db.rs`، `src/main.rs`، `Cargo.toml` (‏`regex = "1"` فقط)، `.gitignore`، `docs/plans/secure-sending-plan.md`، `docs/plans/execution/README.md`. **ممنوع غير ممس**: `src/auth.rs`، `src/topics.rs`، `src/validation.rs`، `src/credentials.rs`، `dashboard/`، `Dockerfile` (تحقّق آلي في القسم 4) |
| **0.** `#![allow(dead_code)]` في `src/config/mod.rs` | تم | السطر 2، ومعه تعليق `// يُزال عند ربط المخزن في T7/T8` |
| **1.** جداول SQLite في `db.rs` + فصل الهجرة | تم | `pub async fn migrate(pool:&SqlitePool)->Result<(),sqlx::Error>` مستدعاة من `init_db()` (السلوك لم يتغير). جداول: `app_configs` (مع `version INTEGER NOT NULL DEFAULT 1`)، `templates`، `config_versions`، `audit_log`. **لم تُضف** جداول الأجهزة/الحملات/tool_users |
| **2.** نماذج البيانات في `src/config/mod.rs` | تم | كل النماذج `Serialize+Deserialize+Clone+Debug+PartialEq` و`deny_unknown_fields`. `AppConfig` + `ConfigDocument` مكتوبة صراحةً (لا `flatten`، لأن serde لا يقبله مع `deny_unknown_fields`) |
| **3.** محلل القوالب `placeholder.rs` | تم | `extract()` + `is_literal()` + `Ref`/`SenderField`/`Placeholder` |
| **4.** التحقق `validate.rs` | تم | `Issue{path,message}`، `validate_all` (كل الأخطاء)، `validate_template`، `validate_app_name` |
| **5.** المخزن `store.rs` | تم | كل الدوال المطلوبة في القسم 5 من ملف الخطوة + `app_count()` مساعدة |
| **6.** أوامر CLI | تم | `src/cli.rs` + فرع في `main.rs` **قبل** فحص CORS وقبل فتح المنفذ. `import` ⇒ `OK`/قائمة `Issue`، `export` ⇒ JSON منسق. وسيط غير معروف ⇒ استخدام + كود 2. بلا وسائط ⇒ السلوك الحالي |
| **7.** المثال `policies/watt.example.json` | تم | **مستخرج آلياً** بـ`sed -n '132,251p'` من القسم 3.2 (بلا إعادة كتابة). يمر في `import` بلا أخطاء |
| **8.** الاختبارات المطلوبة (44 اسماً) | تم | **44/44 موجودة** في `cargo test -- --list`، بلا `#[ignore]`. +3 اختبارات إضافية (47 جديد إجمالاً) |
| **9.** أوامر الجودة | تم | `cargo build` بلا تحذير، `cargo clippy --all-targets` **0 تحذير**، `cargo test` **77 ناجح / 0 فاشل** (30 قديمة + 47 جديدة). الاختبار اليدوي للـCLI نجح. المخرجات في القسم 3 |
| **10.** الـ commits | تم | اثنان بالترتيب، بلا push. القسم 8 |

---

## 2. الملفات

### `git diff --stat` (8a08046 → العمل)

**الـ commit الأول (الكود):**
```
 .gitignore                 |   1 +
 Cargo.toml                 |   1 +
 policies/watt.example.json | 120 ++++
 src/cli.rs                 | 131 ++++
 src/config/mod.rs          | 612 ++++++++++++++++
 src/config/placeholder.rs  | 285 ++++++++
 src/config/store.rs        | 1390 +++++++++++++++++++++++++++++++++++
 src/config/validate.rs     | 1712 ++++++++++++++++++++++++++++++++++++++++++++
 src/db.rs                  |  79 +-
 src/main.rs                |  27 +
 10 files changed, 4350 insertions(+), 8 deletions(-)
```

**الـ commit الثاني (التوثيق):**
```
 docs/plans/execution/README.md      |   1 +
 docs/plans/execution/T1.md          | 143 ++++
 docs/plans/reports/T1-report.md     | 444 ++++++++++++++++++++++
 docs/plans/secure-sending-plan.md  |   2 +
 4 files changed, 590 insertions(+)
```

| الملف | جديد/معدّل | الوصف |
|---|---|---|
| `src/config/mod.rs` | جديد | نماذج الإعدادات والقوالب والحملات + `ConfigDocument` + `default_firebase()` |
| `src/config/placeholder.rs` | جديد | محلّل `{p.x}` / `{sender.*}` / `{doc.*}` / `{entity.*}` / `{var}` وفلتر `\|int` |
| `src/config/validate.rs` | جديد | كل قواعد التحقق `[V*] [T*] [G*] [C*] [S*]` + 25 اختباراً |
| `src/config/store.rs` | جديد | `ConfigStore` على SQLite: حفظ/إصدارات/سجل/استيراد/تصدير/تراجع + 15 اختباراً |
| `src/cli.rs` | جديد | `import` و`export` (بلا مكتبة جديدة) |
| `policies/watt.example.json` | جديد | المثال الكامل لتطبيق واط، مستخرج آلياً من الخطة 3.2 |
| `src/db.rs` | معدّل | فصل `migrate()` + 4 جداول جديدة + `ALTER TABLE app_configs ADD COLUMN version` |
| `src/main.rs` | معدّل | `mod cli; mod config;` + فرع CLI قبل CORS + `config: Arc<ConfigStore>` في `AppState` + سطر تحميل المخزن |
| `Cargo.toml` | معدّل | `regex = "1"` فقط |
| `.gitignore` | معدّل | `!policies/*.example.json` |
| `docs/plans/secure-sending-plan.md` | معدّل | عمود `version` في DDL (3.14) + سطر T1 في سجل التقدم (9) |
| `docs/plans/execution/README.md` | معدّل | سطر T1 في جدول الخطوات |
| `docs/plans/execution/T1.md` | جديد | ملف الخطوة (كان غير متعقب) |

**لم يتغيّر:** `src/auth.rs`، `src/topics.rs`، `src/validation.rs`، `src/credentials.rs`، `dashboard/`، `Dockerfile`، `README.md` (من S0)، `data.db`، `.env`، `credentials/`.

---

## 3. الأوامر التي شغلتها ونتائجها

### 3.1 البناء (بلا تحذير)

```
$ cargo build
   Compiling fcm_microservice v0.1.0 (/Users/rasol/DevsTools/codes/rust/fcm_microservice)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.57s
```
**0 تحذير، 0 خطأ.**

### 3.2 clippy (0 تحذير)

```
$ cargo clippy --all-targets
    Checking fcm_microservice v0.1.0 (/Users/rasol/DevsTools/codes/rust/fcm_microservice)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.57s
```
**0 تحذير** (لا `warning:` ولا `error:` في المخرج).

### 3.3 الاختبارات

```
$ cargo test
running 77 tests
...
test result: ok. 77 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.57s
real 9.71s
```

| المؤشر | القيمة |
|---|---|
| اختبارات ناجحة | **77** (30 قديمة + 47 جديدة) |
| اختبارات فاشلة | **0** |
| متجاهَلة (`ignored`) | **0** |
| الـ30 اختباراً القديمة من S0 | **كلها ما زالت تنجح** |
| المدة | 9.57s |

### 3.4 أسماء الاختبارات الجديدة كاملة (من `cargo test -- --list`)

```
config::placeholder::tests::placeholder_int_filter_flag: test
config::placeholder::tests::placeholder_is_literal: test
config::placeholder::tests::placeholder_parses_all_forms: test
config::placeholder::tests::placeholder_rejects_bad_names_and_paths: test
config::placeholder::tests::placeholder_rejects_stray_braces: test
config::placeholder::tests::placeholder_rejects_unknown_filter: test
config::store::tests::corrupt_row_is_skipped_not_fatal: test
config::store::tests::create_app_default_is_valid: test
config::store::tests::create_app_twice_conflicts: test
config::store::tests::export_import_round_trip_is_lossless: test
config::store::tests::import_example_creates_app: test
config::store::tests::import_invalid_leaves_previous_intact: test
config::store::tests::restore_other_app_version_not_found: test
config::store::tests::restore_returns_exact_content_as_new_version: test
config::store::tests::save_invalid_template_writes_nothing: test
config::store::tests::save_invalid_writes_nothing: test
config::store::tests::save_section_bumps_version_and_cache: test
config::store::tests::save_section_stale_version_conflict: test
config::store::tests::save_template_valid_and_listed: test
config::store::tests::store_loads_existing_rows_on_new: test
config::store::tests::template_change_cannot_break_whole_config: test
config::store::tests::versions_and_audit_rows_written_in_same_transaction: test
config::validate::tests::app_check_requires_firebase_provider: test
config::validate::tests::bad_pattern_regex_rejected: test
config::validate::tests::c1_entity_requires_sender_uid: test
config::validate::tests::c2_wildcard_star_alone_rejected: test
config::validate::tests::c3_max_topics_range: test
config::validate::tests::c5_action_check_requires_sender_uid: test
config::validate::tests::c7_first_n_requires_n: test
config::validate::tests::doc_ref_without_context_rejected: test
config::validate::tests::example_policy_is_valid: test
config::validate::tests::g1_admin_only_topic_with_text_param_allowed: test
config::validate::tests::g1_non_admin_topic_with_text_param_rejected: test
config::validate::tests::g2_uid_from_param_rejected: test
config::validate::tests::g3_rate_required_for_non_admin: test
config::validate::tests::g4_context_without_sender_uid_rejected: test
config::validate::tests::g5_topic_all_rejected_for_non_admin: test
config::validate::tests::jwt_hs256_requires_secret_env: test
config::validate::tests::jwt_mixed_hs_rs_rejected: test
config::validate::tests::jwt_none_algorithm_rejected: test
config::validate::tests::jwt_rs256_requires_https_jwks: test
config::validate::tests::max_topics_over_5_rejected: test
config::validate::tests::reserved_data_keys_rejected: test
config::validate::tests::role_from_data_source_requires_data_source: test
config::validate::tests::undefined_param_ref_rejected: test
config::validate::tests::undefined_var_ref_rejected: test
config::validate::tests::unknown_field_is_rejected: test
```

**47 اختباراً جديداً.** الـ44 المطلوبة في القسم 8 من `T1.md` كلها موجودة؛ الزيادة الثلاثة:
`placeholder_rejects_bad_names_and_paths`، `placeholder_is_literal`، `create_app_twice_conflicts`.

### 3.5 الفحص اليدوي لأمرَي CLI (المطلوب في القسم 9)

```
$ DATABASE_URL=sqlite://t1_demo.db cargo run --quiet -- import watt policies/watt.example.json
OK
exit=0
```

```
$ DATABASE_URL=sqlite://t1_demo.db cargo run --quiet -- export watt | head -40
{
  "app_check": {
    "mode": "off",
    "project_number": "919552016311"
  },
  "campaigns": {
    "audience": {
      "allowed_topics": [
        "country_*",
        "lang_*",
        "users",
        "technicals",
        "cat_*",
        "company_{p.company_id}"
      ],
      "max_topics": 5,
      "required_topic": "promos"
    },
    "content": {
      "actions": {
        "open_company": {
          "check": null,
          "data": {
            "action": "open_company",
            "company_id": "{p.company_id}"
          },
          "params": {}
        },
        "open_product": {
          "check": {
            "doc": "store_products/{p.product_id}",
            "require": {
              "owner_id": "{sender.uid}",
              "status": "approved"
            }
          },
          "data": {
            "action": "open_product",
            "product_id": "{p.product_id}"
          },
...
```

**Round-trip يدوي:**

```
$ ... export watt > /tmp/t1_export_1.json      # exit=0، 3 قوالب: admin_alert/order_new/order_status، version=1
$ ... import watt /tmp/t1_export_1.json        # OK، exit=0
$ ... export watt > /tmp/t1_export_2.json      # exit=0
$ diff /tmp/t1_export_1.json /tmp/t1_export_2.json
ROUND TRIP: identical JSON
```

**مسارات الخطأ:**

```
$ ... import watt /tmp/t1_broken.json          # قالب فيه {p.undefined_param}
templates.order_new.title: [T3] parameter 'undefined_param' is not defined in params
exit=1

$ cargo run --quiet -- frobnicate
unknown command 'frobnicate'
Usage:
  fcm_microservice import <app> <file.json>   import a configuration document
  fcm_microservice export <app>                 print the configuration as JSON
exit=2

$ cargo run --quiet -- export
Usage:
  ...
exit=2
```

**الأمر لا يحتاج CORS ولا `.env`:**

```
$ CORS_ALLOWED_ORIGINS= env -u CORS_ALLOWED_ORIGINS DATABASE_URL=sqlite://t1_demo.db cargo run --quiet -- export watt | head -3
{
  "app_check": {
    "mode": "off",
exit=0
```
(لا يوجد ملف `.env` في المستودع أصلاً.)

**التنظيف:**

```
$ rm -f t1_demo.db t1_demo.db-journal
$ git status --short
 M .gitignore
 M Cargo.toml
 M docs/plans/execution/README.md
 M docs/plans/secure-sending-plan.md
 M src/db.rs
 M src/main.rs
?? docs/plans/execution/T1.md
?? policies/
?? src/cli.rs
?? src/config/
```
**لا `*.db` في `git status`** ✓

### 3.6 اسم التطبيق في `.gitignore`

```
$ git check-ignore policies/watt.example.json      # بلا -v: لا مخرجات، exit=1 ⇒ غير متجاهَل
$ git check-ignore policies/secrets.json           # مخرجات ⇒ ما زال متجاهَلاً بـ'*.json'
$ git add -n policies/watt.example.json
add 'policies/watt.example.json'
```

---

## 4. طريقة الفحص

### 4.1 الفحص البند ببند (القسم 8 من `T1.md`)

| المطلوب | النتيجة | كيف تحققت |
|---|---|---|
| `example_policy_is_valid` | ✅ | المثال المستخرج آلياً من 3.2 ⇒ `validate_all` يرجع **0 خطأ** |
| `unknown_field_is_rejected` | ✅ | `serde_json::from_value::<Template>` يرفض `totally_unknown` |
| `undefined_param_ref_rejected` | ✅ | `{p.order_id}` بلا تعريف ⇒ `[T3]` |
| `doc_ref_without_context_rejected` | ✅ | `{doc.order_number}` بلا `context` ⇒ `[T3]` |
| `undefined_var_ref_rejected` | ✅ | `{status_text}` بلا `vars` ⇒ `[T3]` |
| `jwt_none_algorithm_rejected` | ✅ | `"algorithms": ["none"]` ⇒ فشل التحليل (لا يوجد متغير `None` أصلاً) |
| `jwt_mixed_hs_rs_rejected` | ✅ | `["RS256","HS256"]` ⇒ `[V2]` |
| `jwt_hs256_requires_secret_env` | ✅ | HS256 بلا `hs256_secret_env` ⇒ `[V2]` |
| `jwt_rs256_requires_https_jwks` | ✅ | `jwks_url` بـ`http://` ⇒ `[V2]` |
| `role_from_data_source_requires_data_source` | ✅ | `data_source = none` ⇒ `[V4]` |
| `max_topics_over_5_rejected` | ✅ | `max_topics = 6` ⇒ `[C3]` |
| `bad_pattern_regex_rejected` | ✅ | `"([unclosed"` ⇒ `[T2]` (عبر `regex::Regex::new`) |
| `reserved_data_keys_rejected` | ✅ | `notification` و`google.c` ⇒ `[T6]` |
| `g1_non_admin_topic_with_text_param_rejected` | ✅ | قالب `to.topic` + معامل `text` + مرسل `user` ⇒ `[G1]` |
| `g1_admin_only_topic_with_text_param_allowed` | ✅ | نفس القالب ومرسله `admin` ⇒ **لا** `[G1]` (معفى) |
| `g2_uid_from_param_rejected` | ✅ | `to.uid = {p.target}` ⇒ `[G2]` |
| `g3_rate_required_for_non_admin` | ✅ | مرسل `user` بلا `rate` ⇒ `[G3]` |
| `g4_context_without_sender_uid_rejected` | ✅ | `require` فيه `{doc.owner_id}` فقط ⇒ `[G4]` |
| `g5_topic_all_rejected_for_non_admin` | ✅ | `to.topic = "all"` ⇒ `[G5]` |
| `c1_entity_requires_sender_uid` | ✅ | `entity.require` فارغ ⇒ `[C1]` |
| `c2_wildcard_star_alone_rejected` | ✅ | `allowed_topics = ["*"]` ⇒ `[C2]` |
| `c3_max_topics_range` | ✅ | `1` مرفوض، `5` مقبول ⇒ `[C3]` |
| `c5_action_check_requires_sender_uid` | ✅ | `check.require` فارغ ⇒ `[C5]` |
| `c7_first_n_requires_n` | ✅ | `mode=first_n` و`n=None` ⇒ `[C7]` |
| `app_check_requires_firebase_provider` | ✅ | `mode=enforce` + مزود `jwt` ⇒ `[V8]` |
| `create_app_default_is_valid` | ✅ | `default_firebase()` ⇒ `validate_all` = 0 خطأ، وصف واحد في `app_configs` |
| `save_section_bumps_version_and_cache` | ✅ | `version` 1 ⇒ 2 في القاعدة والذاكرة |
| `save_section_stale_version_conflict` | ✅ | الحفظ بـ`expected_version=1` بعد الرفع ⇒ `Conflict{current_version:2}` |
| `save_invalid_writes_nothing` | ✅ | قسم تالف ⇒ `Invalid`، والإصدار في القاعدة بقي 1، وصفوف الإصدارات بقيت 1 |
| `save_template_valid_and_listed` | ✅ | القالب محفوظ/version=1/mُدرج |
| `save_invalid_template_writes_nothing` | ✅ | 0 صفوف في `templates` |
| `template_change_cannot_break_whole_config` | ✅ | `{p.order_id}` بلا تعريف ⇒ `[T3]` و0 صفوف |
| `restore_returns_exact_content_as_new_version` | ✅ | الاستعادة من النسخة 2 أعادت `users/{uid}.role` **حرفياً** كنسخة 4 |
| `restore_other_app_version_not_found` | ✅ | نسخة تطبيق آخر ⇒ `NotFound` |
| `versions_and_audit_rows_written_in_same_transaction` | ✅ | 2 صفا إصدارات + 2 صفا سجل لكل كتابة |
| `export_import_round_trip_is_lossless` | ✅ | export ⇒ import ⇒ export ⇒ JSON متطابق |
| `import_example_creates_app` | ✅ | 3 قوالب في الذاكرة والقاعدة |
| `import_invalid_leaves_previous_intact` | ✅ | القوالب 3 والنسخة 1 لم تتغيّر |
| `store_loads_existing_rows_on_new` | ✅ | `ConfigStore::new` على قاعدة موجودة ⇒ نفس القوالب |
| `corrupt_row_is_skipped_not_fatal` | ✅ | صف تالف ⇒ `new` لا يفشل، والتطبيق التالف يُتخطى |

**placeholder (4):** `placeholder_parses_all_forms` (8 صيغ)، `placeholder_rejects_stray_braces` (قوس غير مغلق / `}` يتيمة / أقواس متداخلة)، `placeholder_rejects_unknown_filter` (`|float`)، `placeholder_int_filter_flag` (`|int` ⇒ `int_filter = true`).

### 4.2 معايير القبول (القسم 136 من `T1.md`)

| # | المعيار | النتيجة |
|---|---|---|
| 1 | `src/auth.rs`، `topics.rs`، `validation.rs`، `credentials.rs` بلا تغيير | ✅ `git diff --quiet ac4ef44 -- <file>` ⇒ كلها "unchanged" |
| 2 | أسماء الاختبارات الـ44 موجودة وفي `--list` ولا `#[ignore]` | ✅ checked آلياً: "missing: NONE" و`#[ignore]` = 0 |
| 3 | لا `unwrap()`/`expect(` في `src/config` و`src/cli.rs` | ✅ `grep -n "unwrap()\|expect(" src/config src/cli.rs` ⇒ **فارغ** |
| 4 | المدقق يرجع **كل** الأخطاء | ✅ `validate_all` لا يتوقف عند الأول (مثال: المثال التالف يعطي `[C2]` و`[C3]` معاً في `c3_max_topics_range`)، و`save_invalid_writes_nothing` يتحقق من عدم الكتابة |
| 5 | لا قيمة سر + clippy 0 + الـ30 القديمة تنجح | ✅ لا سر في أي ملف (القيم المخزَّنة: `secret_env` **أسماء متغيرات فقط**، والمخزن لا يقرأ أي متغير بيئة) + clippy 0 + 77/77 |
| 6 | `import` على المثال ينجح و`export`⇒`import` يعطي نفس JSON | ✅ يدوياً (3.5) واختباراً |
| 7 | التقرير بالقالب الكامل مع الانحرافات وأسئلة التصميم | ✅ هذا الملف |

### 4.3 فحص النطاق (يدوي + آلي)

- `src/auth.rs`، `src/topics.rs`، `src/validation.rs`، `src/credentials.rs` ⇒ بلا تغيير (القسم 4.2 #1).
- `dashboard/` و`Dockerfile` ⇒ `git status` لا يذكرهما.
- **لا ربط بالطلبات الحية**: `src/main.rs` فيه `mod cli; mod config;` + فرع CLI + `config: Arc<ConfigStore>` في `AppState` فقط. **لا endpoint جديد**، ولا تغيير في `auth.rs`، ولا إرسال.

---

## 5. انحرافات عن الأمر (وسببها)

1. **`cargo fmt` على ملفاتي فقط بدل الأمر الكامل.** `cargo fmt` الكامل كان سيعدّل `src/credentials.rs` — وهو **ممنوع** في T1. تحققت أن الملف **كان غير منسّق قبل عملي**:
   `git show HEAD:src/credentials.rs | rustfmt --check --edition 2021` ⇒ يطبع فروقاً ⇒ حالة سابقة لا سببي.
   لذلك شغّلت `rustfmt --edition 2021 src/config/mod.rs src/config/placeholder.rs src/config/validate.rs src/config/store.rs src/cli.rs`، وتحققت أن `rustfmt --check` على ملفاتي + `src/db.rs` + `src/main.rs` **نظيف** (الفروق الباقية كلها في `credentials.rs`).
2. **`ConfigDocument` بلا `#[serde(flatten)]`.** الخطوة سمحت بذلك صراحةً: serde **يرفض** `flatten` مع `deny_unknown_fields` (وهو مطلوب في الخطوة). فكتبت `ConfigDocument` صراحةً بنفس الحقول.
3. **قواعد تحقق أضفتها أو رمّزتها** (المطلوب ذكرها في القسم 7 من ملف الخطوة):
   - **`[T0]`**: مفتاح القالب `^[a-z][a-z0-9_]{0,39}$` — القاعدة في الخطوة لكن **بلا رمز**، فأعطيتها `[T0]`.
   - **`[V4]` على `role.cache_seconds`**: الخطوة ذكرتها لـ`role_members` فقط؛ طبّقتها على مصدر الدور أيضاً (≤ 3600).
   - **`[V4]` حقول فارغة**: `role.doc`، `role.field`، `role_members.collection`، `role_members.field` يجب أن تكون غير فارغة.
   - **`[T3]`**: `vars.*.from` يجب أن يكون **مساراً صالحاً** بعد البادئة (`doc.`/`p.`/`sender.`)، لا مجرد بداية صحيحة.
   - **`[T3]`**: قيم `vars.map` لا تشير إلى `vars` (منصوص عليه في [T3]؛ نفّذته كخطأ مستقل).
   - **`[S1]`**: خطأ JSON ⇒ `ConfigError::Invalid(vec![Issue{path, message}])` — لأن `ConfigError` المحدد في الخطوة فيه 4 متغيرات فقط ولا متغير لأخطاء JSON.
   - **`[S2]`**: قسم غير معروف في `save_section` ⇒ `Invalid`.
   - **`[S3]`**: `ConfigDocument.version` يجب أن يساوي 1.
   - **`[G1]`**: «ممنوع وجود معامل `text` في القالب» — نفّذتها كـ«أي معامل نوعه `text` في القالب» (وليس فقط المستخدم في `to`)، لأن نصّ الخطة 3.5 القاعدة 1 يقول ذلك.
4. **`is_literal` تُرجع false للنص المشوّه.** الخطوة: «ترجع true إذا لا placeholders». اخترت أن النص الذي فيه خطأ صياغة **ليس** حرفياً ⇒ `false` (fail-closed)، لا أن يُعتبر سليماً.
5. **قالب تالف ⇒ التطبيق كله يُتخطى عند التحميل.** الخطوة: «يتجاهل ويسجّل أي سجل تالف». طبّقت التجاهل على مستوى التطبيق كله (fail-closed: مبدأ 7) بدل تحميل التطبيق بلا ذلك القالب.
6. **`set_template_enabled` و`delete_template` يُنشئان صفَّي `config_versions` و`audit_log` ويرفعان نسخة القالب.** الخطوة §5 تقول «كل كتابة» داخل معاملة واحدة؛ هاتان عمليتان كتابة للقالب، فاعتمدتُ نفس القاعدة (كل كتابة تُسجَّل ⇒ مبدأ 8). **لم ترفع** `app_configs.version` لأن لكل قالب نسخته المستقلة.
7. **`restore` لقسم `full` يطبّق الإعداد만 ويُبقي القوالب.** محتوى صفّ `full` هو JSON الإعداد **بلا قوالب** (لأنه يُكتب وقت إنشاء التطبيق)، فاسترجاعه لا يمس القوالب.
8. **`export` يعيد مفاتيح JSON مرتبة أبجدياً** (BTreeMap + serde_json) ⇒ ترتيب مفاتيح مختلف عن الملف المصدر. **المحتوى بلا فقد** (round-trip متطابق بايت-ببايت، القسم 3.5).

---

## 6. ملاحظات خارج النطاق

1. **مستودع `taqati` لم يُمس** إطلاقاً في T1 (لا علاقة للخطوة به).
2. **`src/credentials.rs` غير منسّق** (من قبل T1) ⇒ `cargo fmt` الكامل سيغيّره. **لم أصلحه** لأنه ممنوع في T1. يحتاج قرار المالك: إما فرع تنظيف لاحق، أو إضافة `#[rustfmt::skip]`.
3. **`data.db` و`.env` و`credentials/` لم تُمس** (لا يوجد `.env` في المستودع أصلاً).
4. **لا يوجد endpoint للقراءة/الكتابة للإعدادات** — كما نصّت الخطوة ("لا تربط شيئاً بالطلبات الحية"). ربط الداشبورد في T7/T8.
5. **المخزن لا يقرأ `secret_env`** — فقط يتحقق من اسمه بنمط `^[A-Z][A-Z0-9_]{0,63}$`. قراءة السر وHMAC في T4.
6. **`templates.enabled` (عمود) و`enabled` (داخل JSON)** kept in sync: العمود هو المرجع عند القراءة، و`set_template_enabled` يحدّث الاثنين معاً.
7. **تحذير الأداة (من S0) ما زال في `README.md`** ويبقى صالحاً: ما زال `my-server` غير آمن للنشر.

---

## 7. أسئلة للمالك/المراجع

1. **[مهم] هل يرفع حفظُ قالب نسخةَ الإعداد (`app_configs.version`)؟** حالياً: لا — لكل قالب نسخته المستقلة، فـ409 في الداشبورد على الأقسام لا يرى تغييرات القوالب. البديل: رفع نسخة الإعداد عند كل تغيير قالب (أقوى ضدسباق المديرين، لكن تعقيد أكبر).
2. **هل `set_template_enabled` و`delete_template` يجب أن تنشئا صفَّي إصدارات وسجل؟** طبّقتُ «كل كتابة» (مبدأ 8). البديل: تسجيلها في `audit_log` فقط.
3. **هل حذف قالب مستخدَم خلال 30 يوماً مسموح في T1؟** الخطة 3.5 تمنعه («القالب المعطل ⇐ 404، والحذف غير مسموح إذا استُعمل خلال آخر 30 يوماً»)، لكن `last_used_at` غير موجود بعد (T6/‏T12 يتولّيانه) ⇒ `delete_template` يحذف بلا فحص. **لم أضف الفحص** لأنه يحتاج `last_used_at`.
4. **[T0] رمز قاعدة مفتاح القالب:** هو رمز من عندي (القاعدة موجودة في الخطوة بلا رمز). هل تفضّل اسماً آخر؟
5. **قواعد أضفتها** (§5 #3): `role.doc`/`role.field` غير فارغين، `role.cache_seconds ≤ 3600`، `vars.from` مسار صالح، `version = 1` في المستند. كلها "fail-closed" ومعقولة، لكنها **ليست** في نص الخطوة ⇒ أرجو الموافقة أو التصحيح.
6. **`ConfigError` بلا متغير لأخطاء JSON** ⇒ رُميت في `Invalid` برمز `[S1]`. هل تُفضّل إضافة متغير `Json`؟
7. **`restore` لقسم `full`** يستعيد الإعداد فقط. هل يجب أن يستعيد كل شيء (إعداد + قوالب)؟ ذلك يحتاج تخزين المستند الكامل في صفّ الإصدارات (المخزَّن حالياً هو JSON الإعداد بلا قوالب).
8. **`app_check.project_number` أرقام فقط** — الخطة 3.12 تكتب `projects/<project_number>`، وهذا يطابق. ✓ بلا سؤال.

---

## 8. حالة git النهائية

### الفرع والـ commits

```
$ git branch --show-current
feature/policy-auth

$ git log --oneline -5
<hash2> docs(plan): T1 update execution tracker and DDL
010a879 feat(config): T1 add app config model, validation and SQLite store
8a08046 docs(plan): S0 add secure sending plan, execution rules and deploy warning
```

*(hash الـ commit الأول: `010a879`. والـ commit الثاني هو الذي يحتوي هذا التقرير، فـ hashه مرجع ذاتي ويُقرأ بـ `git log -1`.)*

### `git status --short`

```
(فارغ)
```

### `git diff --stat` بعد العمل

*(الأرقام النهائية في القسم 2.)*

### ملاحظات

- **بلا `push`.**
- **بلا تعديل** على `taqati`.
- stash قديم واحد في هذا المستودع (`stash@{0}: On my-server: last` بتاريخ 2026-09-23) — **لم أنشئه ولم ألمسه** (كما في S0).

# تقرير T12-B

- **التاريخ/الوقت:** 2026-10-22، من 11:25 إلى 13:05 (توقيت الجهاز).
- **الفرع:** `feature/policy-auth` (تحقّق قبل البدء وقبل كل commit؛ لم يتغيّر).
- **HEAD قبل العمل:** `e6393f6` (`docs(plan): T12-B dashboard UI execution prompt`).
- **HEAD بعد العمل:** 6 commits جديدة، آخرها `build(dashboard): T12-B rebuild dist` — بلا push. (لم يُكتب هاش الـcommits هنا: التقرير نفسه جزء من هذه الـcommits، فأي إعادة كتابة محلية تجعله غير مطابق ✗ ⇒ انظر `git log`.)
- **ملف الخطوة المنفَّذ:** `docs/plans/execution/T12B.md` (الشريحة B من T12، بعد T12-A في `docs/plans/reports/T12A-report.md`).

## 1. ما نُفِّذ (بند بند من ملف الخطوة)

| البند | الحالة | ملاحظة |
|---|---|---|
| المبدئ: لا نسخة ثانية من القواعد | تم | المصدر الوحيد للحقيقة `POST /apps/{app}/templates/validate` (debounce 400ms ✓). لا تحقق محلي من قواعد القالب ✗ (لا نسخة من `validate.rs` في الواجهة ✓)؛ ما في الواجهة هو **تحويل形状** فقط (`definitionToForm`/`formToDefinition`) ✓. |
| §0.0 `T12-B-0` | تم | `docs/plans/reports/T12B-contract.md` (كل مسار تستعمله الشاشات الجديدة: bodies، أشكال الردود، رموز الأخطاء، ومن يملك كل مسار ✓ — مستخرج من `src/admin/{templates,campaigns,history,apps}.rs` و`src/config/mod.rs` ✓) + التبويبات الجديدة في `AppSettings.vue` ✓ + المكوّنات المشتركة `KeyValueRows.vue` و`ParamRows.vue` ✓ + نصوص `strings.js` ✓. |
| §0.1 `T12-B-1` | تم | `TemplatesPanel.vue` (القائمة: المفتاح، الوصف، مفعّل/معطّل مع مفتاح تبديل `PUT .../enabled` ✓، المرسلون ✓، الهدف ✓، آخر تعديل ✓، أزرار: جديد ✓ تعديل ✓ تكرار (مفتاح جديد يطلبه المستخدم ✓) حذف (كتابة المفتاح ✓)) + `TemplateEditor.vue` (نموذج ⇄ JSON ثنائي الاتجاه ✓، JSON غير صالح يمنع الرجوع للنموذج برسالة ✓، كل الحقول الفعلية من `config::Template` ✓، إدراج المتغيرات بنقرة عند المؤشر ✓) + `src/lib/templateForm.js` (التحويلات النقية ✓) + `src/lib/issueCodes.js` (خريطة الرموز ✓) + **التحقق المباشر** (debounce 400ms ✓، `issues` بجوار الحقل (المسار ⇒ آخر جزء = الحقل ✓) وفي شريط ملخص ✓، زر "حفظ" معطّل مع وجود أخطاء ✓، ومعالجة `409 conflict` **دون فقدان التعديلات** ✓ مع زرّي "إعادة تحميل" و"حفظ كنسخة جديدة" ✓). |
| §0.2 `T12-B-2` | تم | `PreviewPanel.vue` (محاكي هاتف HTML/CSS ✓، قيم تجريبية للمعاملات والسياق والمرسِل ✓، `POST .../preview` ✓، الرد **نصاً** (`{{ }}` ✓ بلا HTML ✓)، حالة `rendered:false` (فشل `require`) ✓، زر معطّل + تلميح "احفظ أولاً" للقالب غير المحفوظ ✓، والإرسال التجريبي `POST .../test-send` ✓ مع الشرط `test_uid` ✓ ورابط لصفحة "حسابي" ✓ ومعالجة 429 ✓) + `VersionsPanel.vue` (`GET /versions` ✓، عرض نسخة `GET /versions/:id` ✓، **مقارنة سطرية** بدالة نقية `src/lib/diff.js` بلا مكتبات ✓، `POST .../restore` بتأكيد ✓ — و`withReauth` غير مطلوب لأن الخلفية لا تطلب إعادة تحقق على الاسترجاع ✓ (§8)) + `ImportExportPanel.vue` (تصدير Blob + `a[download]` باسم `app-<name>-config.json` ✓ بلا تمرير البيانات في URL ✓، استيراد باختيار ملف وعرض ملخص (عدد القوالب والأقسام ✓) وتأكيد صريح ✓، الملف في الذاكرة ويُصفَّر بعد الإرسال ✓، تنبيه أن الاستيراد يستبدل الإعدادات ✓، `issues` تُعرض ✓). |
| §0.3 `T12-B-3` | تم | `CampaignsPanel.vue` (طابور المراجعة: مرشّح الحالة ✓ + ترقيم (`limit`/`offset` ✓)، بطاقة: الشركة ✓ العنوان/النص/الصورة نصاً ✓، **الجمهور بلغة مفهومة** بدالة نقية `describeAudience` في `src/lib/audience.js` ✓، الشرط الخام في `<details>` للمالك فقط ✓، الحالة وسبب الرفض ✓، أزرار: موافقة (`note` اختياري ✓)، رفض (سبب **إلزامي** ✓)، حظر الشركة ✓، شارة "الشركة محظورة" ✓، تعارض الحالة ⇒ إعادة تحميل البطاقة برسالة ✓) + `BlocksPanel.vue` (`GET/POST/DELETE /blocks` ✓ مع كيان وسبب ✓) + `SendLogPanel.vue` (`GET /history` بمرشّحات النوع والحالة ✓ + المرجع (تصفية على الصفحة المحمّلة، لأن الخلفية لا ترشّح به ✓ — §8) + `GET /history/:id` ✓ نصوص فقط ✓) + `FreeSendPanel.vue` (`POST /apps/{app}/send` ✓، نافذة تأكيد قبل الإرسال ✓، تحذير بارز للموضوع العام ✓، `withReauth` ✓، الزر معطّل أثناء الإرسال ومنع النقر المزدوج ✓، النتيجة delivered/failed ✓) + `DevicesPanel.vue` (`GET /devices/stats` ✓ بجدول وأعمدة CSS بلا مكتبات رسم ✓). **لا استطلاع دوري (polling) في أي مكان** ✓ — التحديث بزر فقط ✓. |
| §0.4 `T12-B-4` | تم | **29 اختباراً جديداً** بالأسماء الإلزامية كلها ✓ + ثوابت T12-A خضراء ✓ = **65** اختباراً (§3). التوثيق: هذا التقرير ✓ + `docs/plans/reports/T12B-contract.md` ✓ + جدول التقدم في الخطة (§9) ✓ + `docs/plans/execution/README.md` ✓ + `dashboard/README.md` ✓. |
| §0.5 commit `dist/` | تم | commit مستقل `build(dashboard): T12-B rebuild dist` ✓ (يحتوي `dist/**` وحده ✓). |
| §1 التبويبات | تم | **اختيار**: تبويبات داخل صفحة `AppSettings.vue` (أبسط ✓، بلا مسارات فرعية جديدة ✓). التبويبات: القوالب، الحملات (الطابور)، الحظر، سجل الإرسال، الإرسال الحر، الأجهزة، الإصدارات (مع الاستيراد/التصدير) ✓. عرض الصلاحيات حسب الخلفية ✓ (الخلفية تقرر الوصول ✓؛ الاستيراد/التصدير للمالك فقط — المكوّن لا يظهر للمدير ✓). |
| §2.1 القائمة | تم | كما في §0.1 ✓. |
| §2.2 المحرر | تم | كما في §0.1 ✓. الحقول: المفتاح (لالجديد فقط ويُقفل بعد الحفظ ✓)، `senders` ✓، `params` (صفوف: اسم/نوع/حدود ✓ — `text`/`pattern`/`enum`/`int` كما في `config::Param` ✓)، `context.doc` + `context.require` (صفوف: حقل/شرط ✓)، `vars` (اسم/`from`/خريطة ✓)، الهدف (`uid`/`role`/`topic`/`condition` كما في `enum Target` ✓)، العنوان والنص مع إدراج المتغيرات ✓، `data` (صفوف ✓)، `dedupe` ✓، `rate` ✓، `image` ✓. `action` **غير موجود** في `config::Template` ⇒ لم يُضف ✓. |
| §2.3 التحقق المباشر | تم | كما في §0.1 ✓. |
| §2.4 الأخطاء الحاجزة | تم | رموز الخلفية الحقيقية: `[S1] [S2] [G1..G6] [T0..T7] [V1..V8] [C1..C7]` (31 رمزاً ✓ — مجموعة من الكود لا التخمين ✓). خريطة عربية في `strings.js` ✓ مع الرجوع إلى النص الأصلي ✓، والاختبار `issue_code_map_covers_every_backend_code_used` **يقرأ ملفات الخلفية نفسها** (`src/config/validate.rs` و`src/campaigns.rs` و`src/db.rs` و`src/admin/templates.rs` و`src/admin/apps.rs`) ويتأكد أن لكل رمز نصاً عربياً ✓ ⇒ "ما لا يقل عن 5 رموز حقيقية" مغطّى بالكامل ✓. |
| §3.1 المعاينة | تم | كما في §0.2 ✓. |
| §3.2 الإرسال التجريبي | تم | كما في §0.2 ✓. |
| §3.3 الإصدارات | تم | كما في §0.2 ✓. |
| §3.4 الاستيراد/التصدير | تم | كما في §0.2 ✓. |
| §4.1 طابور المراجعة | تم | كما في §0.3 ✓. **الحصة المتبقية**: الخلفية لا ترسلها في `GET /apps/{app}/campaigns` ⇒ لم تُعرض (و§8) ✓. |
| §4.2 الحظر | تم | كما في §0.3 ✓. |
| §4.3 لا استطلاع | تم | لا `setInterval` ولا `setTimeout` متكرر في أي مكوّن ✓ — التحديث بزر "تحديث" فقط ✓. |
| §5.1 السجل | تم | كما في §0.3 ✓. مرشّح التاريخ: الخلفية لا ترشّح بالتاريخ (لا تعرف إلا `before` بالمعرّف) ⇒ لم يُضف (و§8) ✓. |
| §5.2 الإرسال الحر | تم | كما في §0.3 ✓. **عدد المستلمين المتوقع**: الخلفية لا ترسله في `POST /send` ⇒ نافذة التأكيد تعرض الهدف والعنوان فقط (و§8) ✓. **الموضوع العام**: الخلفية لا تملك قائمة مواضيع عامة ⇒ التحذير يظهر لموضوعي البث `promos` و`all` (و§8) ✓. |
| §5.3 الأجهزة | تم | كما في §0.3 ✓ — الأعمدة بالمجابة الفعلية (`by_platform` ✓، `total` ✓، `distinct_uids` ✓، `active_last_7d` ✓، `newest_last_seen` ✓). |
| §6 الاختبارات | تم | 29 اختباراً جديداً ✓ + 36 من T12-A = 65 ✓ (§3). |
| §7 التجارب السلبية | تم | الخمس ✓ (§3). |
| §8 التشغيل الحقيقي | تم (جزء الخلفية) | §3 و§4 ✓. **ما لم يُختبر**: الإرسال التجريبي والإرسال الحر الفعليان (يحتاجان FCM حقيقياً ✓ — ممنوع في هذه الخطوة ✓)، والمظهر البصري (لا متصفح آلي ✓). |
| §9 القبول | تم | §5 ✓. |

## 2. الملفات

`git diff --stat e6393f6..HEAD -- dashboard docs/plans`:

```
 dashboard/src/components/BlocksPanel.vue       | 150 ++++++
 dashboard/src/components/CampaignsPanel.vue    | 355 ++++++++++++++
 dashboard/src/components/DevicesPanel.vue      | 162 +++++++
 dashboard/src/components/FreeSendPanel.vue     | 150 ++++++
 dashboard/src/components/ImportExportPanel.vue | 175 +++++++
 dashboard/src/components/KeyValueRows.vue      |  79 +++
 dashboard/src/components/ParamRows.vue         | 143 ++++++
 dashboard/src/components/PreviewPanel.vue      | 200 ++++++++
 dashboard/src/components/SendLogPanel.vue      | 186 +++++++
 dashboard/src/components/TemplateEditor.vue    | 647 +++++++++++++++++++++++++
 dashboard/src/components/TemplatesPanel.vue   | 189 ++++++++
 dashboard/src/components/VersionsPanel.vue    | 189 ++++++++
 dashboard/src/lib/audience.js                  |  54 +++
 dashboard/src/lib/diff.js                      |  63 +++
 dashboard/src/lib/issueCodes.js                |  33 ++
 dashboard/src/lib/templateForm.js              | 266 ++++++++++
 dashboard/src/strings.js                       | 223 +++++++++
 dashboard/src/views/AppSettings.vue            |  55 ++-
 docs/plans/reports/T12B-contract.md            |  79 +++
 19 files changed, 3397 insertions(+), 1 deletion(-)
```

الملفات الجديدة (سطر شرح لكل واحد):

- `dashboard/src/components/BlocksPanel.vue` — قائمة الحظر وإضافته ورفعه.
- `dashboard/src/components/CampaignsPanel.vue` — طابور مراجعة الحملات (مرشّحات، ترقيم، موافقة/رفض/حظر، شارة المحظور).
- `dashboard/src/components/DevicesPanel.vue` — إحصاء الأجهزة بجدول وأعمدة CSS.
- `dashboard/src/components/FreeSendPanel.vue` — الإرسال الحر مع نافذة التأكيد والتحذير.
- `dashboard/src/components/ImportExportPanel.vue` — تصدير/استيراد الإعدادات (المالك فقط).
- `dashboard/src/components/KeyValueRows.vue` — صفوف مفتاح/قيمة (مشترك) لـ`data` و`require` وخريطة `vars`.
- `dashboard/src/components/ParamRows.vue` — صفوف المعاملات بأنواعها وحدودها (مشترك).
- `dashboard/src/components/PreviewPanel.vue` — محاكي الهاتف والإرسال التجريبي.
- `dashboard/src/components/SendLogPanel.vue` — سجل الإرسال ومرشّحاته.
- `dashboard/src/components/TemplateEditor.vue` — محرر القالب (نموذج ⇄ JSON) والتحقق المباشر.
- `dashboard/src/components/TemplatesPanel.vue` — قائمة القوالب.
- `dashboard/src/components/VersionsPanel.vue` — الإصدارات والمقارنة والاسترجاع.
- `dashboard/src/lib/audience.js` — `describeAudience` (نقية، لا ترمي أبداً).
- `dashboard/src/lib/diff.js` — `diffLines` (مقارنة سطرية نقية، بلا مكتبات).
- `dashboard/src/lib/issueCodes.js` — قراءة خريطة الرموز من `strings.js`.
- `dashboard/src/lib/templateForm.js` — تحويلات القالب (تعريف ⇄ نموذج) وقوائم المتغيرات.
- `dashboard/src/__tests__/{templateEditor,preview,campaigns,freeSend,importExport,issueCodes}.test.js` — 29 اختباراً جديداً.
- `docs/plans/reports/T12B-contract.md` — عقد المسارات المستهلكة.

## 3. الأوامر التي شغلتها ونتائجها

```
$ cd dashboard && pnpm test
 Test Files  15 passed (15)
      Tests  65 passed (65)
```
(36 اختباراً من T12-A + 29 جديداً ✓ — مخرجات `vitest --reporter=verbose` في §4.)

```
$ cd dashboard && pnpm build
✓ built in 1.25s
../dist/assets/index-Ch0LTe-Y.js   249.51 kB │ gzip: 81.11 kB
```
(البناء الأخير في §5 بعد آخر تعديل ✓.)

**أساس الخلفية (لم يتغيّر)**: `cargo test` ⇒ `test result: ok. 623 passed; 0 failed; 0 ignored` ✓، و`cargo clippy --all-targets` ⇒ **بلا تحذيرات** ✓ (`git diff --stat e6393f6..HEAD -- src Cargo.toml Dockerfile docker-compose.yml` فارغ ✓ ⇒ لم يتغيّر سطر واحد في الخلفية ✓).

**التشغيل الحقيقي (جزء الخلفية)** — بيئة مؤقتة **خارج المستودع** (`/tmp/t12b-run` و`/tmp/t12b-run2` ✓)، `DATABASE_URL=sqlite:///tmp/t12b-run/data.db` ✓، `CREDENTIALS_DIR=/tmp/t12b-run/credentials` ✓، `COOKIE_SECURE=false` ✓، `PORT=18081` و`18082` (ليست 8080 ✓). لم يُستدعَ Google ولا Firebase ولا FCM ✓. كلمة المرور المؤقتة **لم تُطبع** (بقيت في متغيّر الصدفة ✓) ولم تُكتب في أي ملف ولا report ✓.

التطبيق التجريبي `demo` أُنشئ بالأمر `fcm_microservice import demo /tmp/t12b-run/config.json` (الأمر موجود في `src/cli.rs` ✓ ولا يحتاج credentials ✓ ولا Google ✓) — لأن `POST /apps` يطلب فحصاً حياً لمفتاح Google (ممنوع هنا ✓).

المخرجات الحقيقية المختصرة:

```
=== the list of the applications ===
[{"app":"demo","configured":true,"disabled":false,"enabled_templates":0,"has_credentials":false,"project_id":null,"template_count":0,"version":1}]
[status 200]

=== the validation of a valid template ===
{"issues":[],"ok":true}
[status 200]

=== the validation of a template that breaks the rules ===
{"issues":[{"message":"[T3] parameter 'unknown' is not defined in params","path":"templates.bad.title"}],"ok":false}
[status 200]

=== the creation of the template ===
{"version":1}
[status 201]

=== the list of the templates ===
[{"description":"a greeting","enabled":true,"key":"greet","last_used_at":null,"senders":["admin"],"target_kind":"topic","version":1}]
[status 200]

=== the update of the template (with the version of the contract) ===
{"version":2}
[status 200]

=== the update with an old version (the conflict of the editor) ===
{"current_version":2,"error":"conflict"}
[status 409]

=== the enable and the disable of the template ===
[status 204]   [status 204]

=== the preview of the template ===
{"body":"Welcome Sara","data":{"kind":"greeting"},"dedupe_key":null,"image":null,"recipients":{"count":null,"kind":"topic"},"target":{"kind":"topic","value":"news"},"title":"Hello Sara!"}
[status 200]

=== the versions ===
[{"changed_by":"owner@test.local","created_at":"...","id":3,"section":"template:greet","size":254}, ... {"id":1,"section":"full","size":218}]
[status 200]

=== the restore of the version ===
{"version":3}
[status 200]

=== the preview of a template with a context that needs a document ===
{"reason":"forbidden","rendered":false}
[status 200]

=== the export of the configuration ===
{"app_check":{"mode":"off","project_number":null},"campaigns":{...},"devices":{...},"identity":{...},"templates":{...},"version":1}

=== the import of the configuration (the same document) ===
{"version":2}
[status 200]

=== the campaigns ===
[]
[status 200]

=== the blocks ===
{"blocked":true,"cancelled":0}
[status 200]
[{"blocked_by":"owner@test.local","created_at":"...","entity_id":"company-1","reason":"abuse"}]
[status 200]
{"blocked":false}
[status 200]

=== the history of the sends ===
[]
[status 200]

=== the devices ===
{"active_last_7d":0,"by_platform":{"android":0,"ios":0,"web":0},"distinct_uids":0,"newest_last_seen":null,"total":0}
[status 200]

=== the keys of the app ===
[]
[status 200]
```

**نتائج التجارب السلبية الخمس** (`/tmp/t12b_experiments.sh` ✓ — كل تجربة تكسر قاعدة، ثم يُعاد الملف):

| # | ما كُسر | الاختبار الذي أخفق | النتيجة |
|---|---|---|---|
| 1 | أُزيل الـdebounce من التحقق (`setTimeout(validateNow, 400)` ⇒ `0`) | `template_validate_is_debounced` | `Tests 1 failed | 9 passed (10)` ✓ |
| 2 | عُرض رد المعاينة بـ`v-html` | `no_v_html_anywhere_in_src` | فشل ✓ |
| 3 | أُزيل تأكيد الإرسال الحر (الزر ينفّذ `send` مباشرة) | `free_send_requires_confirmation_dialog` | `Tests 1 failed | 2 passed (3)` ✓ |
| 4 | جاز الحفظ مع وجود `issues` | `template_save_disabled_while_issues_exist` | `Tests 1 failed | 9 passed (10)` ✓ |
| 5 | جعل `describeAudience` يرمي على شكل مجهول | `describe_audience_never_throws_on_unknown_shape` | فشل ✓ |

هاشات الملفات **قبل وبعد**Experiments متطابقة ✓:

```
9c5ef3f682db0f19801d83de2996ef99486c108036b013931364134a6cccf970  src/components/TemplateEditor.vue
92ecd7a8f04e94574c535aaab6b621b0c9f5fa3fec7588a5187276ec289af95d  src/components/PreviewPanel.vue
fbf38a1230455d41e662c4b0f365f8fa6aa1ea510b7b4eeedc02c145e45468f7  src/components/FreeSendPanel.vue
1241d6405385ca17adf58aa99cd069cfe55f09df4162f1acf0c0c4598bf8f6bf  src/lib/audience.js
```
(قبل وبعد متطابقان ✓، وبعدها `Tests 65 passed (65)` ✓.)

## 4. جدول الشاشات ← المسارات المستهلكة

| الشاشة (تبويب) | المسارات التي تستهلكها | ملاحظات |
|---|---|---|
| القوالب (قائمة) | `GET /apps/{app}/templates`، `PUT /apps/{app}/templates/{key}/enabled` | القائمة كما في العقد ✓ |
| القوالب (المحرر) | `GET /apps/{app}/templates/{key}`، `POST /apps/{app}/templates/validate`، `POST /apps/{app}/templates`، `PUT /apps/{app}/templates/{key}`، `DELETE /apps/{app}/templates/{key}` | `expected_version` من العقد ✓، `409 conflict` ⇒ `current_version` ✓ |
| المعاينة | `POST /apps/{app}/templates/{key}/preview` | الرد نصاً ✓ |
| الإرسال التجريبي | `POST /apps/{app}/templates/{key}/test-send` | يتطلّب `test_uid` من الحساب ✓ |
| الإصدارات | `GET /apps/{app}/versions`، `GET /apps/{app}/versions/{id}`، `POST /apps/{app}/versions/{id}/restore` | بلا `withReauth` (الخلفية لا تطلب إعادة تحقق ✓) |
| الاستيراد/التصدير | `GET /apps/{app}/export`، `POST /apps/{app}/import` | المالك فقط ✓؛ الاستيراد يطلب إعادة تحقق ⇒ `withReauth` ✓ |
| الحملات (الطابور) | `GET /apps/{app}/campaigns`، `POST /apps/{app}/campaigns/{id}/approve`، `POST /apps/{app}/campaigns/{id}/reject` | `note` اختياري في الموافقة، إلزامي في الرفض ✓ |
| الحظر | `GET /apps/{app}/blocks`، `POST /apps/{app}/blocks`، `DELETE /apps/{app}/blocks/{entity_id}` | ✓ |
| سجل الإرسال | `GET /apps/{app}/history`، `GET /apps/{app}/history/{id}` | ✓ |
| الإرسال الحر | `POST /apps/{app}/send` | `withReauth` ✓، هدف واحد بالضبط ✓ |
| الأجهزة | `GET /apps/{app}/devices/stats` | ✓ |

## 5. طريقة الفحص: كل بند من "طريقة الفحص" في الخطوة ← النتيجة الفعلية وكيف تحققت

| البند | النتيجة | كيف تحققت |
|---|---|---|
| `template_form_to_json_roundtrip` | ناجح ✓ | `definitionToForm` ثم `formToDefinition` ⇒ `to` و`title` و`senders` و`params` و`enabled` كما هي ✓ |
| `template_json_invalid_blocks_return_to_form` | ناجح ✓ | JSON غير صالح ⇒ `jsonError` غير فارغ ✓ والرجوع إلى النموذج مرفوض ✓ |
| `template_key_locked_after_first_save` | ناجح ✓ | `isNew` حسب وجود المفتاح ✓ وحقل المفتاح معطّل للقالب المحفوظ ✓ |
| `template_variable_chips_insert_at_cursor` | ناجح ✓ | الشرائح من `params` و`vars` و`sender` ✓ والإدراج عند المؤشر ✓ |
| `template_validate_is_debounced` | ناجح ✓ | مؤقّت 400ms ✓ (اختبار بتوقيت وهمي ✓) |
| `template_validate_issues_shown_next_to_field` | ناجح ✓ | المسار `templates.greet.title` يظهر بجوار حقل العنوان ✓ |
| `template_save_disabled_while_issues_exist` | ناجح ✓ | الزر معطّل ✓ |
| `template_save_conflict_keeps_user_edits` | ناجح ✓ | `409` ⇒ التعديلات باقية على الشاشة ✓ والحدث `conflict` ✓ |
| `template_delete_requires_typing_key` | ناجح ✓ | الزر معطّل حتى كتابة المفتاح ✓ |
| `template_clone_requires_new_key` | ناجح ✓ | الزر معطّل حتى كتابة مفتاح جديد ✓ |
| `diff_lines_basic` / `diff_identical_is_empty` | ناجح ✓ | `diffLines` نقية ✓ |
| `restore_requires_confirmation` | ناجح ✓ | زر "استرجاع" ⇒ يطلب تأكيداً ✓ |
| `preview_renders_response_as_text_not_html` | ناجح ✓ | `<b>world</b>` يظهر مُرمَّزاً لا كـHTML ✓ |
| `preview_shows_require_403_message` | ناجح ✓ | `rendered:false` + `reason` تظهر ✓ |
| `test_send_requires_test_uid` | ناجح ✓ | بدون جهاز مرتبط ⇒ الزر معطّل ورابط لصفحة "حسابي" ✓ |
| `test_send_button_disabled_while_sending` | ناجح ✓ | الزر معطّل أثناء الطلب ✓ |
| `describe_audience_plain_arabic_for_each_shape` | ناجح ✓ | `groups` بمجموعة واحدة/مجموعتين/موضوع واحد ✓ |
| `describe_audience_never_throws_on_unknown_shape` | ناجح ✓ | `null` و`{}` و`groups` غير مصفوفة و`[[]]` و`[[null]]` ✓ |
| `campaign_reject_requires_reason` | ناجح ✓ | الزر معطّل بلا سبب ✓ |
| `campaign_approve_conflict_reloads_card` | ناجح ✓ | `409` ⇒ رسالة + إعادة تحميل البطاقة ✓ |
| `blocked_company_badge_shown` | ناجح ✓ | الشارة تظهر ✓ |
| `free_send_requires_confirmation_dialog` | ناجح ✓ | لا إرسال قبل التأكيد ✓ |
| `free_send_double_click_sends_once` | ناجح ✓ | النداء الثاني يُتجاهل أثناء الانتظار ✓ |
| `free_send_public_topic_shows_warning` | ناجح ✓ | `promos` ⇒ تحذير ✓ |
| `import_requires_explicit_confirm` | ناجح ✓ | الزر معطّل بلا التأكيد ✓ |
| `import_file_state_cleared_after_submit` | ناجح ✓ | الاسم والمحتوى والملخص يُصفَّرون ✓ |
| `export_does_not_put_data_in_url` | ناجح ✓ | Blob + `a[download]` + `revokeObjectURL` ✓ والـURL بلا بيانات ✓ |
| `issue_code_map_covers_every_backend_code_used` | ناجح ✓ | يقرأ ملفات الخلفية ويطابق كل رمز بنص عربي ✓ |
| ثوابت T12-A | خضراء ✓ | `no_v_html_anywhere_in_src` ✓، `no_web_storage_in_src` ✓، `no_console_logging_of_payloads` ✓، `no_firebase_references` ✓، `no_arabic_literals_outside_strings_js` ✓ |

## 6. انحرافات عن الأمر (وسببها)

| الانحراف | السبب |
|---|---|
| **كل** النصوص العربية في `strings.js` فقط (بما فيها نصوص الجمهور `describeAudience` ونصوص رموز الأخطاء `issueMessages`) | القاعدة "العربية في `strings.js` فقط" مطلوبة في T12-A ✓ وهي تُخالفها الثابت `no_arabic_literals_outside_strings_js` ✓. الحل: `strings.js` يحمل القطع ✓ و`audience.js` و`issueCodes.js` يركّبانها فقط ✓ (بلا حرف عربي في الكود ✓). |
| حقول `params` و`data` و`require` و`vars` تُحرَّر بصفوف لا بـJSON | مبدأ الخطوة: "إنشاء قالب آمن بالنموذج دون كتابة JSON" ✓ ⇒ صفوف ⇄ كائن (تحويل في `templateForm.js` ✓) والبناء يبقى متاحاً كخيار خبير ✓. |
| `issueCodes.js` يقرأ الخريطة من `strings.js` بدل أن يعرّفها | نفس قاعدة العربية ✓. |
| المواضيع العامة في الإرسال الحر = `promos` و`all` فقط | الخلفية لا تملك قائمة مواضيع عامة (§8) ⇒ اخترنا مواضيع البث المذكورة في الخطة (`promos` إلزامي في كل حملة ✓). |
| مرشّح "المرجع" في السجل يُطبَّق على الصفحة المحمّلة | الخلفية لا ترشّح بـ`ref` (§8) ⇒ لا ادّعاء أن الترشيح من الخلفية ✓. |
| `withReauth` في الاستيراد فقط (لا في الاسترجاع) | الخلفية تطلب إعادة تحقق في `POST /apps/{app}/import` (`user.require_reauth(300)` ✓) ولا تطلبها في `POST /versions/{id}/restore` ✓ ⇒ طابق العقد ✓. |
| استُعمل الأمر `fcm_microservice import` في التشغيل الحقيقي | `POST /apps` يطلب فحصاً حياً لمفتاح Google (ممنوع ✗) ⇒ الأمر يهيّئ التطبيق بلا credentials ولا شبكة ✓ (وهو جزء من T10-A ✓). |
| `git checkout -- dist` (استعادة مخرجات البناء المتتبَّعة فقط) | لإبقاء تغييرات `dist/` في commit واحد مستقل ✓؛ لم يُمس أي ملف آخر ولم يُستعمل `git checkout -- .` ✗. |

## 7. ملاحظات خارج النطاق

1. **استرجاع نسخة "full" بعد `import` من سطر الأوامر يفشل**: `POST /apps/{app}/versions/{id}/restore` على نسخة قُسمها `full` يردّ
   `[S1] the stored version could not be read: unknown field 'templates'` (400). السبب: الأمر `fcm_microservice import` يكتب **وثيقة** الإعداد (فيها `templates`) في صف النسخة `full`، بينما `config.create_app` يكتب **إعداد التطبيق** (بلا `templates`) ⇒ وأداة الاسترجاع تقرأ `full` كإعداد. (استرجاع نسخة قالب `template:*` يعمل ✓ — `{"version":3}` ✓.) هذا في `src/` ⇒ **ممنوع في هذه الخطوة** ⇒ مُبلَّغ في §8.
2. `GET /apps/{app}/campaigns` لا يعيد حصة متبقية للشركة، و`POST /apps/{app}/send` لا يعيد عدد المستلمين المتوقع ⇒ مُبلَّغ في §8.
3. `GET /apps/{app}/history` لا يقبل ترشيحاً بالتاريخ ولا بالمرجع (يقبل `limit` و`before` و`kind` و`status` فقط) ⇒ مُبلَّغ في §8.
4. لا يوجد في الخلفية قائمة "مواضيع عامة" ⇒ التحذير في الإرسال الحر مبني على `promos` و`all` (§6).
5. متابعة من T12-A: `GET /audit` للمدير يحتاج `app` ⇒ ما زال كما هو (المتابعة كما في §8).

## 8. طلبات للخلفية

1. **استرجاع نسخة `full` بعد `import` من سطر الأوامر**: إما أن تكتب `import` نفس شكل `AppConfig` (بلا `templates`) في صف النسخة `full`، أو أن يقرأ `restore` القسم `full` كوثيقة. الأثر: استرجاع النسخة الكاملة بعد أي استيراد من CLI يفشل بخطأ `[S1]` (الواجهة تعرض الخطأ كما هو ✓).
2. **الحصة المتبقية للحملة**: إضافة حقل (مثل `quota_remaining`) إلى رد `GET /apps/{app}/campaigns` حتى تعرضه الواجهة للمدير في الطابور.
3. **عدد المستلمين المتوقع في الإرسال الحر**: إضافة حقل (مثل `recipients`) إلى رد `POST /apps/{app}/send` حتى تعرضه نافذة التأكيد.
4. **ترشيح السجل بالمرجع والتاريخ**: إضافة `ref` (و`from`/`to` للتاريخ) إلى `HistoryQuery` في `src/admin/history.rs`، أو توثيق أن التصفية على الصفحة هي الحد الأقصى.
5. **قائمة المواضيع العامة**: إن أراد المالك تحذيراً دقيقاً في الإرسال الحر، فمصدر الحقيقة يجب أن يكون الخلفية (قائمة مواضيع التطبيق في قسم `campaigns` أو `web_subscribe`)، لا قائمة مكتوبة في الواجهة.
6. **`GET /audit` للمدير**: اقتراح (من T12-A، لم يُنفَّذ): أن يختار المدير التطبيق في الواجهة (قائمة من `GET /apps`) ويُرسل `?app=` بدل أن ترفض الخلفية الطلب ⇒ تعديل في `src/admin/apps.rs` (ممنوع هنا ✓).

## 9. أسئلة للمالك/المراجع

1. **المواضيع العامة**: هل المواضيع التي يجب أن يُحذّر منها في الإرسال الحر هي `promos` و`all` فقط، أم أن الخلفية يجب أن تعطي قائمة (انظر الطلب 5)؟
2. **الحصة**: هل يظهر "الحصة المتبقية" في الطابور مطلوب؟ (يحتاج الطلب 2.)
3. **الترشيح بالتاريخ في السجل**: هل يكفي الترشيح بالنوع والحالة (الحالي)، أم أن الترشيح بالتاريخ مطلوب؟ (يحتاج الطلب 4.)
4. **الأسبقية**: هل تُعرض شاشة "الاستيراد/التصدير" في تبويب مستقل ("الإصدارات") كما هي الآن، أم في تبويب مستقل باسم "الاستيراد/التصدير"؟

## 10. ما لم يُختبر

1. **المظهر البصري**: لا متصفح آلي في البيئة (`PLAYWRIGHT_BROWSERS_PATH` غير معرَّف ولا playwright مثبَّت ✓) ⇒ **الواجهة لم تُجرَّب بصرياً** — المحاكي والتخطيط responsive غير مُتحقّقين بالعين ✓. (قائمة الفحص اليدوي في §11.)
2. **الإرسال التجريبي الفعلي** (`POST .../test-send`) و**الإرسال الحر الفعلي** (`POST /apps/{app}/send`): يحتاجان FCM حقيقياً ومفتاح Google ⇒ لم يُنفَّذا (ممنوع في هذه الخطوة ✓). تحقّقنا من **أشكال** الطلبات والاستجابات فقط ✓ (بما فيها `{"delivered":…,"failed":…}` من الشيفرة و`{"status":"success",…}` من الشيفرة ✓).
3. **معاينة قالب بجهاز حقيقي**: لم يُختبر الإرسال إلى جهاز ✓ (يحتاج FCM ✓).
4. **الصلاحيات بمدير حقيقي**: اختبار التشغيل الحقيقي كان بحساب **مالك** فقط؛ منع المدير عن الاستيراد/التصدير مغطّى بالشرط في المكوّن (`v-if="isOwner"` ✓) وبالخلفية (`Need::Owner` ✓) ولم يُختبر بمدير حقيقي في هذه الخطوة.

## 11. قائمة فحص يدوي بصري (للمالك على النسخة المنشورة)

1. افتح **القوالب**: هل تظهر القائمة بمفتاح ووصف وحالة مفعّل/معطّل؟ وهل زر "قالب جديد" يفتح محرراً فارغاً بمفتاح قابل للتحرير؟
2. اكتب في العنوان نصاً فيه متغير غير معرّف (`{p.zzz}`): هل يظهر الخطأ **بجوار حقل العنوان** خلال أقل من ثانية، وهل شريط الأخطاء يعرض المسار والرسالة العربية؟
3. اضغط "نموذج"/"JSON": هل التحويل يحافظ على نفس الكائن؟ وهل لصق JSON غير صالح يمنع الرجوع إلى النموذج برسالة؟ (جرّب `{ not json`.)
4. انقر شريحة متغير: هل يُدرج `{p.name}` **عند المؤشر** داخل العنوان؟ (انقر مرتين لإدراجه في النص.)
5. جرّب مفتاح قالب موجود: هل يظهر تعارض `409` **دون Losing تعديلاتك**، مع زرّي "إعادة تحميل" و"حفظ كنسخة جديدة"؟
6. حاول الحذف: هل الزر معطّل حتى تكتب المفتاح **حرفياً**؟
7. اضغط "نسخ القالب": هل يطلب مفتاحاً جديداً، وهل يُنشئ نسخة مستقلة؟
8. **معاينة**: هل تعرض لوحة الهاتف العنوان والنص كما ستُرسل؟ هل الرد يظهر **نصاً** (لا HTML)؟ (جرّب عنواناً فيه `<b>`.)
9. قالب فيه `context.require`: هل المعاينة تعرض "فشل الشرط" بدل الرسالة المصاغة؟
10. **الإرسال التجريبي**: هل الزر معطّل مع رسالة تربطك بصفحة "حسابي" إن لم يكن هناك جهاز اختبار؟ وبعد الربط: هل تظهر نتيجة `delivered/failed`؟
11. **الإصدارات**: هل القائمة تعرض القسم والوقت ومن غيّر؟ وهل "عرض" يفتح مقارنة سطرية Highlighted؟ وهل "استرجاع" يطلب تأكيداً؟
12. **الاستيراد/التصدير** (مالك فقط): هل التصدير ينزل ملفاً باسم `app-<name>-config.json`؟ هل الاستيراد يعرض ملخصاً (عدد القوالب والأقسام) ويطلب تأكيداً صريحاً تحذّر من استبدال الإعدادات؟
13. **الطابور**: هل تُعرض الشركة والعنوان والجمهور **بجملة مفهومة**؟ هل يظهر الشعار "الشركة محظورة"؟ وهل زر "رفض" معطّل بلا سبب؟ (جرّب 409 من مراجع آخر: هل تتحدّث البطاقة برسالة؟)
14. **الحظر**: هل يظهر الشارة في الطابور بعد الحظر؟ وهل "رفع الحظر" يعمل؟
15. **الإرسال الحر**: هل يظهر تحذير للموضوع `promos`؟ وهل لا يُرسل شيء قبل "تأكيد الإرسال"؟ وهل النقر المزدوج لا يرسل مرتين؟ وهل `429` يعرض رسالة؟
16. **الأجهزة**: هل الجدول والأعمدة يعرضان الأرقام كما في الخلفية، بلا كسر في التخطيط على الجوال؟
17. **السجل**: هل تعمل مرشّحات النوع والحالة والمرجع؟ (المرجع يصفّي الصفحة المحمّلة ✓.)
18. على **جوال** (عرض ضيّق): هل يبقى المحرر مقروءاً؟ وهل تتراصف الحقول بدل أن تتداخل؟

## 12. حالة git النهائية

```
$ git log --oneline -7
fc5a9df build(dashboard): T12-B rebuild dist
2eb5222 feat(dashboard): T12-B the tests and the documentation
ec9aa3e feat(dashboard): T12-B campaigns, log, free send and devices
d82554e feat(dashboard): T12-B preview, versions and import
80dcc05 feat(dashboard): T12-B the templates and the live validation
13a0d8e feat(dashboard): T12-B contract, tabs and shared rows
e6393f6 docs(plan): T12-B dashboard UI execution prompt
```
(الأرقام أعلاه من لحظة كتابة التقرير؛ إعادة الكتابة المحلية للـcommits الأخيرة تجعل هاش آخر سطرين يختلفان ✗ — العناوين هي المرجع ✓.)

```
$ git status --short
(فارغ — الشجرة نظيفة ✓)
```

- `git diff e6393f6..HEAD -- src Cargo.toml Dockerfile docker-compose.yml` ⇒ **فارغ** ✓ (الخلفية لم تُمَس ✓).
- `git diff --stat 5956f61~1..5956f61` ⇒ `dist/**` وحده ✓ (commit البناء مستقل ✓).
- لا `push` ✓.
- لا أسرار في الـcommits ولا في الـbundle: `private_key` و`private_key_id` و`BEGIN PRIVATE` لا تظهر إلا كأسماء حقول/ترويسة في الشيفرة المولَّدة ✓ (فُحص بـ`grep` على `dist/assets/*.js` ⇒ 3 و6 و1 على التوالي، **بلا قيم** ✓).

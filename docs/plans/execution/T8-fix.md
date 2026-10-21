# إصلاح T8-F — تحرير مفتاح dedupe عند كل خروج مبكر + تنظيف dead_code

**اقرأ أولاً:** `docs/plans/execution/RULES.md` و`docs/plans/reports/T8-report.md`. الفرع `feature/policy-auth` (تحقق قبل البدء وقبل كل commit). قاعدة اللغة كما سبق (إنجليزي في `src/`).
**الملف المسموح:** `src/main.rs` (دالة `send_template` وما يلزمها)، `src/template.rs` (حذف `#![allow(dead_code)]` فقط)، `src/devices.rs` (حذف `#[allow(dead_code)]` التي صارت زائدة فقط، وتعليقاتها)، `docs/plans/**`. ممنوع غير ذلك.

## المشكلة (وجدتها في المراجعة)
في `send_template`، بعد نجاح `claim_dedupe` (الخطوة 9) توجد خروجات مبكرة بخطأ بلا `release_dedupe`:
- `tokens_for_uid(...).map_err(device_error)?` ⇒ 503.
- `role_members` غير مضبوط ⇒ 400، و`source_for(...)` ⇒ 503، و`tokens_for_role(...)` ⇒ 503.
- `project_map.get(app)` غير موجود ⇒ 500.
النتيجة: طلب فاشل بـ503/500 يترك المفتاح محجوزاً، فإعادة المحاولة خلال `seconds` ترجع `duplicate` والإشعار لا يصل أبداً. يجب أن يُحرَّر المفتاح في **كل** مسار لا يصل فيه إشعار.

## المطلوب
1. أعد هيكلة بعد الخطوة 9 بحيث يمر كل خروج بخطأ عبر تحرير المفتاح: الأبسط دالة داخلية `async fn deliver(...) -> Result<Outcome, (StatusCode,String)>` تحتوي الخطوات 10–11، والمستدعي يحرّر المفتاح عند `Err` (وعند `delivered == 0` كما اليوم). لا تكرر `release_dedupe` يدوياً في كل فرع (قابل للنسيان). النتيجة النهائية للردود تبقى كما هي حرفياً.
2. تحرير المفتاح لا يجب أن يخفي الخطأ الأصلي (فشل التحرير يُتجاهل بـ`let _` كما الآن).
3. احذف `#![allow(dead_code)]` من `src/template.rs`. شغّل `cargo build`: إن ظهر تحذير dead_code لعنصر لم يُستعمل بعد (مثل المعامل `entity`) فأعد `#[allow(dead_code)]` على **ذلك العنصر وحده** مع تعليق "used by campaigns (T9)". كذلك امسح `#[allow(dead_code)]` من `devices.rs` حيث صار العنصر مستعملاً فعلاً (أبقِ ما زال غير مستعمل).

## الاختبارات الجديدة (بهذه الأسماء، بنمط اختبارات HTTP الحالية)
- `dedupe_released_when_device_lookup_fails` (قالب بـ`dedupe`، هدف `uid`، تعطيل قراءة الأجهزة بإسقاط جدول `devices` داخل الاختبار أو أي طريقة نظيفة تجعل `tokens_for_uid` يفشل ⇒ 503؛ ثم أعد الجدول/الحالة وأعد الطلب نفسه ⇒ يجب أن **لا** يرجع `duplicate`).
- `dedupe_released_when_role_source_unavailable` (هدف `role` ومصدر بيانات يعطل ⇒ 503؛ إعادة الطلب بعد إصلاح المصدر لا ترجع `duplicate`).
- `dedupe_released_when_role_members_missing` (400 ثم إصلاح الإعداد وإعادة الطلب).
- `dedupe_released_when_credentials_missing` (500: تطبيق له إعداد بلا credentials، ثم تحميل الـ credentials ⇒ إعادة الطلب تنجح).
- `dedupe_still_blocks_after_successful_delivery` (حماية من الإفراط في التحرير: بعد نجاح الإرسال يبقى المفتاح محجوزاً ⇒ الطلب الثاني `duplicate`).

## تجربة سلبية ملزمة
أزل التحرير من مسار `tokens_for_uid` فقط مؤقتاً ⇒ يجب أن يفشل `dedupe_released_when_device_lookup_fails`. ثم أعده وتحقق `sha256` قبل/بعد.

## الجودة والـ commits
`cargo fmt` (ملفاتك)، `cargo build` و`cargo clippy --all-targets` صفر تحذير، `cargo test` كامل (330 + 5 جديدة، لا ignore).
commit كود: `fix(send): T8-F release the dedupe key on every non-delivering exit` (≤72 حرفاً). commit تنظيف: `chore: T8-F drop stale dead_code allows`. commit توثيق: `docs(plan): T8-F update tracker` يتضمن `T8-fix.md` و`T8-fix-report.md` بالقالب، ويحدّث README (T8=✅ بقرار المراجع بعد الإصلاح).

## قرارات المراجع على انحرافات T8 (للتوثيق فقط)
1. سطرا `test_state()` في `devices.rs` (اختبارات) مقبولان.
2. `AppState.identity` يبقى عليه `#[allow(dead_code)]` حتى يستعمله T11/T10.
3. مفتاح الخدمة يتخطى `senders` وحد uid: **مقبول** (باك إند موثوق حسب 3.1). تنبيه للتوثيق: قالب فيه `require` يعتمد `{sender.uid}` سيرد 403 للمفتاح لأن uid هو `service`؛ أضف هذه الملاحظة لقسم القوالب في خطة T13.
4. طول عنوان commit T8-3 مقبول هذه المرة فقط؛ الالتزام بـ72 حرفاً مستمر.
5. ملاحظات التقرير خارج النطاق (مخبأ الأدوار يُبطل عند كل حفظ قالب، و`validate_template` متساهل مع `{var}` في `require`): سجّلها في بند "متابعات" بخطة T13/T10 ولا تعالجها هنا.

## معايير القبول
1. الفرق ضمن الملفات المسموحة فقط؛ الردود/الرموز لم تتغير (اختبارات T8 كلها تمر دون تعديل).
2. لا يوجد `release_dedupe` مكرر بعدد الفروع (أقرأ الكود)، والخروج بخطأ كله يمر عبر نقطة واحدة.
3. الأسماء الخمسة موجودة، والتجربة السلبية موثقة، و`#![allow(dead_code)]` غير موجود في `template.rs`.

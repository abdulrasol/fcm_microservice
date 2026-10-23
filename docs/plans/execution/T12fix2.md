# T12-fix2 — التطبيق غير المُهيَّأ + حذف التطبيق + إضافة التطبيق

## المشكلة (مثبتة بفحص المتصفح على fcm.subultech.top)
1. `GET /api/v1/admin/apps/{app}` يرجع 404 للتطبيق الذي له credentials ولا إعداد له (قرار T10-A المقبول).
   `AppSettings.vue` يعامل أي 404 على أنه "غير موجود" ولا يعرض زر "تهيئة التطبيق" (الشرط `configured && !configured.configured` لا يتحقق أبداً لأن `info=null`).
2. بعد "إضافة تطبيق" ينتقل إلى `/apps/{app}` فيرى "غير موجود" (نفس السبب) فيبدو أن الإضافة فشلت.
3. بعد "حذف التطبيق" ينادي `load()` ويبقى في نفس الصفحة بدل العودة إلى القائمة.

## المطلوب (واجهة فقط، لا تغيير في src/ ولا في عقد الـ API)
في `dashboard/src/views/AppSettings.vue`:
- حالة جديدة `unconfigured`: عندما يرجع `GET /apps/{app}` بـ 404 **ويرجع** `GET /apps/{app}/credentials` بـ 200 → التطبيق موجود وغير مُهيَّأ. لا تعرض "غير موجود"؛ اعرض الشريط `settings.notConfigured` مع زر `settings.init` (المالك فقط) الذي ينادي `POST /apps/{app}/init` ثم `load()`.
- "غير موجود" تظهر فقط إذا فشل الاثنان (لا إعداد ولا credentials).
- في حالة `unconfigured` أخفِ تبويبات الإعداد (الهوية، data_source، ...، الحظر، سجل الإرسال، الإرسال الحر، الأجهزة) وأبقِ "نظرة عامة" والمفاتيح (credentials، تعطيل، حذف، استبدال).
- بعد حذف ناجح: `router.replace('/')` (لا `load()`).
- بعد `init` ناجح: رسالة `initDone` ثم تحميل الإعداد.
- لمدير غير مالك في حالة unconfigured: رسالة أن المالك يجب أن يهيّئ التطبيق (نص جديد في strings.js بالعربية).

في `AddApp.vue`: بعد نجاح الإضافة اذهب إلى `/apps/{app}` كما هو (سيعمل الآن مع الحالة الجديدة)؛ لا تغيير آخر.

## الاختبارات (vitest)
- 404 على التطبيق + 200 على credentials ⇒ يظهر زر التهيئة ولا يظهر "غير موجود"؛ الضغط ينادي POST init مرة واحدة.
- 404 + 404 ⇒ "غير موجود".
- حذف ناجح ⇒ `router.replace('/')`.
- تجربة سلبية إلزامية: أعد الشرط القديم وتأكد أن اختبار التهيئة يفشل، ثم أرجعه.

## التسليم
- أعد بناء `dashboard/dist` وأضفه في commit منفصل `build(dashboard): T12-fix2 rebuild dist`.
- commit للكود: `fix(dashboard): handle unconfigured apps, redirect after delete` (≤72 حرفاً).
- تقرير في `docs/plans/reports/T12fix2-report.md` (بالعربية). لا push.

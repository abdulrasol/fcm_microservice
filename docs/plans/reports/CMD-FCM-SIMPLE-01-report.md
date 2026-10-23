# تقرير CMD-FCM-SIMPLE-01

- التاريخ: 2026-10-23
- الحالة: مكتمل ما عدا إعادة تشغيل `cargo test` (توقف الوكيل بحد الاستخدام)؛ أُكمل الفحص والتقرير يدوياً.
- Git: كل التعديلات unstaged، بلا commit أو stash أو reset.

## ما نُفذ (واجهة فقط)
- `templateForm.js`: `detectTemplateKind` (admin_alert / order_notice / custom) و`applyTemplateKind`، وحفظ التعريف المصدر لضمان أن فتح قالب وحفظه دون تعديل يعيد JSON مطابقاً حرفياً.
- `TemplateEditor.vue` و`ParamRows.vue`: اختيار نوع القالب، حقل «من يرسل؟» لإشعار الطلب، قسم «إعدادات متقدمة» مطوي، وافتراضات `rate` (10 للأدمن، 30 للطلب) و`dedupe` لإشعار الطلب.
- `issueCodes.js` و`strings.js`: رسالة عربية لكل رمز موثّق في القسم 8، والرمز بخط صغير بجانبها.
- `AppSettings.vue`: الحملات تحت «إضافات متقدمة».
- `docs/templates-reference.md`: قسم «الأنواع الجاهزة».
- `dist/` أُعيد بناؤه: `assets/index-BBqBA0rV.js` و`assets/index-DQlr7_L4.css` (index.html يشير إليهما).

## التحقق
- Vitest: 100 ناجحاً (قبل إيقاف الوكيل)، البناء نجح.
- `src/`: نفس الملفات الستة التي عدّلها DIAG-01 فقط (apps.rs, history.rs, mod.rs, db.rs, main.rs, topics.rs)؛ لا Rust جديد، لذا نتيجة `cargo test` السابقة (696) تبقى سارية.
- لم أستطع تشغيل Vitest من بيئة المراجعة (ثنائيات rolldown لماك)، فالرقم 100 من تقرير الوكيل.

## قيود
- مرجع القوالب فيه تعريفات كاملة لـ order_new وorder_status ومثال admin_alert فقط؛ كشف النوع يعتمد على البنية الأمنية الدقيقة، وأي قالب لا يطابق تماماً يُفتح «مخصصاً». اللوحة الحية تُظهر أن القوالب العشرة تطابق (أدمن: senders الأربعة، to.role=admin؛ طلب: order_id + require + to.uid).

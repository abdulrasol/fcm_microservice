# W4-fix3 — إلغاء المشتري للطلب يُنتج 403 (taqati)

## السبب (من سجل التشغيل: `notAuthorized 403` في `OrderDetailsScreen._confirmCancelOrder`)
`ApiOrderNotifier.notifyStatusChanged` يستدعي دائماً القالب `order_status`، وهذا القالب مخصص لإشعار **المشتري** ويشترط `seller_user_id == {sender.uid}`.
عندما **المشتري نفسه** يلغي الطلب يكون هو المرسل فيفشل الشرط ⇒ 403 (سلوك القالب صحيح؛ الخلل أن التطبيق اختار القالب الخطأ). والمطلوب في هذه الحالة إشعار **البائع** بالإلغاء.

## ما أُنجز في الأداة (لا تنشئه)
قالب جديد `order_cancelled` في الداشبورد لتطبيق watt: المرسلون user/company/technical، `params.order_id`، `context.require.buyer_id == {sender.uid}`، الهدف `{doc.seller_user_id}`، العنوان "إلغاء الطلب #…"، المتن "ألغى {buyer_name} الطلب #…"، `dedupe` بمفتاح `{order_id}:cancelled` لـ 10 دقائق.

## المطلوب (taqati)
1. `NotificationTemplates.orderCancelled = 'order_cancelled'`.
2. غيّر `OrderNotifier.notifyStatusChanged` ليستقبل `actorUserId` (من `UpdateOrderStatusUseCase` الذي يملكه أصلاً) أو احسب الدور داخل المنفّذ بمقارنة `actorUserId` مع `order.buyerId`.
3. المنطق في `ApiOrderNotifier`:
   - `newStatus == cancelled` و`actorUserId == order.buyerId` ⇒ أرسل `order_cancelled {order_id}` (يصل للبائع).
   - غير ذلك (البائع غيّر الحالة) ⇒ `order_status {order_id}` كما الآن (يصل للمشتري).
   - إن كان المنفّذ هو الأدمن (لا هو البائع ولا المشتري) فلا ترسل إشعاراً (سجّل dPrint فقط) لأن القالبين سيرفضانه.
4. اختبارات `api_order_notifier_test.dart`: مشتري يلغي ⇒ القالب `order_cancelled`؛ بائع يؤكد/يشحن ⇒ `order_status`؛ بائع يلغي/يرفض ⇒ `order_status`؛ أدمن ⇒ لا إرسال. **تجربة سلبية:** أعد الاختيار الثابت لـ `order_status` وأثبت فشل اختبار المشتري.
5. `flutter analyze` و`flutter test` نظيفان، `dart format --line-length 160`.

## التسليم
مهمة `W4-FIX3` في TODO.md، تقرير `docs/plans/reports/W4-FIX3-report.md`، رسالة commit جاهزة بلا تنفيذ.

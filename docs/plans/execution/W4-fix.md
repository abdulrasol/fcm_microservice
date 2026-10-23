# W4-fix — قاعدة Firestore تتحقق من `seller_user_id` عند إنشاء الطلب (taqati)

## المشكلة
`firestore.rules` يسمح للمشتري بإنشاء `store_orders` بأي محتوى (يشترط فقط `buyer_id == uid` و`status == 'pending'`). قالب `order_new` يرسل الإشعار إلى `{doc.seller_user_id}` وبنصٍّ من `buyer_name`/`total` في الوثيقة. عميل معدّل يستطيع كتابة الطلب مباشرة بـ `seller_user_id` لأي uid وباسم مشتري مزوّر فيصل إشعار "طلب جديد" مزيّف لأي مستخدم (حتى 30 مرة/ساعة). التحقق في `FirebaseOrderRemoteDataSource.createOrder` ليس حماية لأنه في العميل.

## المطلوب
1. **قاعدة الإنشاء:** أضف شروطاً لـ `allow create` في `match /store_orders/{orderId}`:
   - `request.resource.data.seller_user_id == get(/databases/$(database)/documents/companies/$(request.resource.data.seller_id)).data.user_id`
   - `request.resource.data.buyer_id == request.auth.uid` (موجود) و`request.resource.data.status == 'pending'` (موجود).
   - `request.resource.data.keys().hasAll(['seller_id','seller_user_id','buyer_id','buyer_name','total','currency','order_number'])`.
   - لا يجوز أن يكون `seller_user_id == request.auth.uid` (لا طلب من الشخص لنفسه).
   - تحقق مرجعي: `buyer_name` يطابق `get(users/{uid}).data.full_name`؛ **إن لم يوجد هذا الحقل بهذا الاسم** فتحقق من الاسم الفعلي في `users` من الكود (`user.dart`) ولا تخمّن. إن تعذّر التحقق أبقِ الشرط خارج القاعدة ودوّنه في التقرير بدل كسر الإنشاء.
2. **قاعدة التحديث:** لا يستطيع أي طرف تغيير `seller_user_id` ولا `seller_id` ولا `buyer_id` بعد الإنشاء (`request.resource.data.seller_user_id == resource.data.seller_user_id` عند وجوده)، ولا تغيير `status` إلى قيمة غير معرّفة في `OrderStatus`.
3. **طلبات قديمة بلا `seller_user_id`:** التحديث عليها ما زال يعمل عبر شرط `seller_id == uid` الحالي، ولا نكسر قراءتها. للإنشاء الجديد فقط يُشترط الحقل.
4. **اختبارات الـ Emulator (إلزامية، وشغّلها فعلاً):** عدّل `firestore_rules_tests/tests/store_orders_rules.test.js`:
   - إنشاء بـ `seller_user_id` صحيح من وثيقة الشركة ⇒ مسموح.
   - `seller_user_id` لمستخدم آخر ⇒ مرفوض. بدون الحقل ⇒ مرفوض. شركة غير موجودة ⇒ مرفوض.
   - المشتري يغيّر `seller_user_id` بعد الإنشاء ⇒ مرفوض. البائع يغيّر `status` ⇒ مسموح. الغريب ⇒ مرفوض.
   - **تجربة سلبية:** احذف شرط المطابقة مؤقتاً وأثبت أن اختبار "uid آخر" يفشل.
   - ضع في التقرير ناتج التشغيل الفعلي (`firebase emulators:exec` أو الأمر المعتمد في المستودع). إن تعذّر التشغيل فاذكر السبب ولا تدّعِ النجاح.
5. تحقق أن `submit_order_usecase`/`createOrder` ما زالا يكتبان كل الحقول المشترطة (اختبار وحدة: الوثيقة المكتوبة تحتوي المفاتيح).

## ما لا تفعله
- لا تنشر القواعد (`firebase deploy`): نشرها خطوة المالك بعد مراجعة SEC3.
- لا تغيّر القوالب ولا الأداة.

## التسليم
مهمة `W4-FIX` في TODO.md، تقرير `docs/plans/reports/W4-FIX-report.md`، ورسالة commit جاهزة بلا تنفيذ.

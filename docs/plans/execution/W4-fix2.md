# W4-fix2 — استعلامات قوائم الطلبات للبائع بعد تشديد القواعد + تقييد الحقول (taqati)

> مراجعة W4-FIX (commit 02bc0e6) وجدت 3 مشاكل. نفّذها بنفس القواعد (`AGENTS.md` و`TODO.md` مهمة `W4-FIX2`) ولا تنشر القواعد.

## المشاكل (مثبتة من قراءة `firestore.rules` والكود)
1. **قد تنكسر قائمة طلبات البائع (أخطر):** `firebase_order_remote_data_source.dart:111,133` يستعلم `where('seller_id', isEqualTo: sellerId)` (والواجهة تمرر `user.id`).
   قاعدة القراءة القديمة كانت تحوي `resource.data.seller_id == request.auth.uid` وهي قابلة للإثبات من شرط الاستعلام، وW4-FIX **حذفها**. الفرعان المتبقيان يعتمدان على `'seller_user_id' in resource.data` و`get(companies/...)`، وهذه لا يمكن لـ Firestore إثباتها من شروط استعلام `list` ⇒ `PERMISSION_DENIED` على الاستعلام كله.
   اختبارات القواعد الحالية تفحص `get` على وثيقة واحدة فقط، لا استعلامات.
   (ملاحظة: معرّف وثيقة الشركة يُولَّد تلقائياً في `firebase_company_data_source.dart:23` فقط أحياناً يساوي uid، لذا الاستعلام بـ `seller_id == uid` لا يطابق طلبات `seller_id = companyId` أصلاً.)
2. **ادّعاء غير منفّذ:** التقرير يقول "المشتري يلغي فقط وهو pending" لكن القاعدة لا تفحص `resource.data.status == 'pending'`؛ المشتري يستطيع إلغاء طلب `shipped/delivered`.
3. **الحقول غير مقيّدة في التحديث:** البائع أو المشتري (عند الإلغاء) يستطيع تغيير `total` و`items` و`buyer_name` وغيرها في نفس الـ update.

## المطلوب
### أ) قواعد (`firestore.rules`)
- أعد فرع القراءة القابل للإثبات: `resource.data.seller_id == request.auth.uid` (يغطي حالة معرّف شركة = uid) **بالإضافة** إلى `resource.data.seller_user_id == request.auth.uid` (بلا شرط `'seller_user_id' in resource.data` المربك: استخدم `resource.data.get('seller_user_id', null) == request.auth.uid`).
- أبقِ فرع الطلبات القديمة عبر `get(companies/...)` للقراءة المفردة فقط.
- تقييد التحديث بـ `request.resource.data.diff(resource.data).affectedKeys().hasOnly([...])`:
  - البائع: `['status','updated_at','last_updated_by','status_update_note']`.
  - المشتري (إلغاء): نفس المفاتيح، وبشرط `resource.data.status == 'pending'`.
  - تحقق من الأسماء الفعلية للحقول في `store_order_model.dart` (لا تخمّن) وأضف أي حقل مشروع يكتبه الكود فعلاً.
- مسموح للأدمن بكل شيء كما هو.
### ب) الكود
- استعلامات البائع (`getSellerOrders` وستريمه): استعلم بـ `seller_user_id == uid`، ثم ادمج نتيجة استعلام ثانٍ `seller_id == uid` للطلبات القديمة (إزالة التكرار بالمعرّف، الترتيب بالأحدث)، واجعل فشل الاستعلام الثاني غير قاتل (يُسجَّل فقط). لا يتغير عقد الـ repository/domain.
- إن لزم فهرس مركب أضفه في `firestore.indexes.json` وسمّه في التقرير.
### ج) اختبارات Emulator (شغّلها فعلاً)
- **استعلامات list**: `where('seller_user_id','==',uid)` من البائع ⇒ مسموح؛ `where('seller_id','==',uid)` ⇒ مسموح؛ استعلام بـ uid مستخدم آخر ⇒ مرفوض؛ استعلام المشتري `buyer_id==uid` ⇒ مسموح.
- البائع يغيّر `total` ⇒ مرفوض؛ يغيّر `status` فقط ⇒ مسموح. المشتري يلغي طلب `pending` ⇒ مسموح؛ طلب `shipped` ⇒ مرفوض؛ يغيّر `total` أثناء الإلغاء ⇒ مرفوض.
- **تجربة سلبية:** احذف فرع `seller_id == uid` مؤقتاً وأثبت أن اختبار استعلام `seller_id` يفشل، ثم أعده.
- اختبار وحدة Dart (fake/mocktail) لدمج النتيجتين وإزالة التكرار.
### التسليم
مهمة `W4-FIX2` في TODO.md، تقرير `docs/plans/reports/W4-FIX2-report.md` بنتائج Emulator الفعلية، ورسالة commit جاهزة بلا تنفيذ. لا تنشر القواعد.

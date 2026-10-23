# تقرير T19

- التاريخ/الوقت: 2026-10-23 17:15:22 +03
- الفرع: `my-server`
- HEAD قبل التنفيذ: `50d98ba`
- HEAD بعد تنفيذ الكود وقبل إرفاق هذا التقرير: `bef6dd8`

## 1. ما نُفِّذ

| البند | الحالة | ملاحظة |
|---|---|---|
| نقل نافذة إعادة التحقق خارج `Shell` | تم | أصبح `Shell` ونافذة `role=dialog` عنصرين شقيقين، وبقي `router-view` الوحيد داخل `Shell`. |
| إنهاء الوعد عند الإلغاء وحارس غياب الحوار | تم | الإلغاء يرفض بـ`cancelled`، وغياب الحوار يرفض بـ`reauth_unavailable` بدل تعليق الوعد. |
| اختبار `App` كاملًا مع `POST /users` | تم | يثبت ظهور حقل كلمة المرور، إعادة الطلب مرة واحدة بعد كلمة المرور، وعودة زر الإنشاء فعالًا بعد الإلغاء. |
| عرض فشل الإرسال الحر | تم | الخطأ يستخدم `.error`، و502 يعرض «فشل التسليم إلى FCM» مع `reason` و`http_status` إن وجدا، ولا تُعرض استجابة `status=failed` كنتيجة نجاح. |
| مهلة طلب الواجهة | تم | `timeout: 40000` لطلب الإرسال الحر، والزر يعود فعالًا في `finally`. |
| تشخيص الخادم | تم | تصنيف مغلق `auth`/`network`/`rejected`، وإضافة `http_status` للرفض، وتسجيل مقتطف أول 200 حرف من جسم FCM في tracing فقط. |
| حفظ سبب الفشل في السجل | تم | يخزن `reason` و`http_status` داخل `notifications_history.response` من دون جسم FCM الخام. |
| مهلة مسارات الأدمن | تم | الإرسال الحر و`test_send` محاطان بمهلة 10 ثوانٍ، والمهلة تصنّف `network`. |
| اختبارات النقل الوهمي | تم | غطت Auth وNetwork وRejected بحالتي 404 و400، وحفظ السبب، وعدم تسريب الجسم الخام، ومهلة الإرسال. |
| إعادة بناء `dist/` | تم | Vite بنى 140 وحدة في 341ms، في commit مستقل. |

## 2. الملفات

إحصاء `git diff --stat 50d98ba..bef6dd8`: 14 ملفًا، 661 إضافة و88 حذفًا. أضيف ملف الاختبار `dashboard/src/__tests__/appReauth.test.js` لاختبار المسار الكامل، وأعيد توليد ملفات `dist/assets` ذات الأسماء المبنية على المحتوى. هذا التقرير هو الملف الجديد الآخر.

## 3. الأوامر ونتائجها

- `pnpm test`: نجح، 20 ملف اختبار و95 اختبارًا، 0 فشل.
- `cargo test admin::history::tests::free_send_ -- --nocapture`: نجح، 16 اختبارًا، 0 فشل.
- التجربة السلبية `cargo test admin::history::tests::free_send_auth_failure_reports_auth -- --nocapture`: فشلت عمدًا بعد إسقاط Auth مؤقتًا؛ عاد 200 بدل 502. أُعيد الكود الصحيح بعدها.
- `cargo test` داخل sandbox: 588 نجحت و100 فشلت بسبب `Operation not permitted` عند فتح منافذ loopback وغياب Keychain؛ لا تمثل فشلًا في الكود.
- `cargo test` خارج sandbox: نجح، 688 اختبارًا، 0 فشل.
- `cargo fmt -- --check`: نجح بلا فروق.
- `cargo clippy --all-targets --all-features -- -D warnings`: نجح بلا تحذيرات.
- `cargo build`: نجح بلا تحذيرات.
- `scripts/check_*`: نجحت فحوص الأسرار (13 ملفًا)، متغيرات البيئة (13 موثقة و15 متحققًا منها)، الروابط (20 رابطًا/70 ملفًا)، والصلاحيات (65 مسارًا).
- `pnpm build`: نجح؛ 140 وحدة، وخرج `index-CORj9NTP.js` و`index-Da8QULCL.css`.
- `git diff --check`: نجح بلا أخطاء مسافات.

## 4. طريقة الفحص

- 403 `reauth_required` على `POST /users`: اختبار `appReauth.test.js` ركب `App` و`Shell` و`Users`، فوجد حوار كلمة المرور، ثم تحقق من استدعاء `/users` مرتين فقط (الأول وretry واحد) ومن استدعاء `/auth/reauth` بكلمة المرور.
- الإلغاء: الاختبار نفسه تحقق من اختفاء التعطيل عن زر «إنشاء مدير» وعدم إعادة `POST /users`.
- حارس الأمان: `reauth_without_a_dialog_rejects_instead_of_hanging` تحقق من `reauth_unavailable`.
- 502 في الواجهة: اختبار `free_send_502_is_red_and_includes_the_server_reason` تحقق من `.error` والنص والسبب والحالة والمهلة 40000 وعودة `busy=false`.
- `status=failed`: اختبار مستقل تحقق من بقاء `result=null`.
- أسباب الخادم والسجل: اختبارات history تحققت من `auth` و`network` و`rejected`، ومن 404 و400، ومن وجود القيم في `response` وعدم وجود جسم FCM الخام.
- منع رجوع الإسقاط الصامت: التجربة السلبية فشلت في assertion الحالة عند حذف تصنيف Auth مؤقتًا.

## 5. انحرافات عن الأمر

- لا يوجد انحراف وظيفي. أضيف حقل داخلي `ApiError.data` حتى تستطيع الواجهة قراءة حقول جسم 502 المطبّع.
- شُغلت مجموعة Rust الكاملة مرة ثانية خارج sandbox لأن الاختبارات تستخدم خوادم loopback وشهادات النظام.

## 6. ملاحظات خارج النطاق

- التشخيص الحي لبيئة الخادم وسبب فشل FCM الفعلي لم يُنفذ، التزامًا بنطاق T19.

## 7. أسئلة للمالك/المراجع

- لا توجد.

## 8. حالة git النهائية

قبل إرفاق التقرير كان `git status --short` فارغًا. آخر commits للكود:

```text
bef6dd8 fix(admin): report why a free send failed
cfcdabc build(dashboard): T19 rebuild dist
afae868 fix(dashboard): show send failures as errors
4508a49 fix(dashboard): render the reauth dialog outside the shell
50d98ba docs(plans): W4-fix2 list-query and field restriction prompt
```

لم يُنفذ `git push`.

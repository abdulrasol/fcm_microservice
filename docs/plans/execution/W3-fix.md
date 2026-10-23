# W3-fix — تنبيهات التسجيل وتغيير الدور تفشل بصمت (taqati)

## المشكلة
في `complete_profile_screen.dart` و`edit_profile_page.dart` (وبقية مواضع W3) الإرسال محاط بـ `try { ... } catch (_) {}` فارغ، فأي رفض (400/403/503) يختفي تماماً. المستخدم لا يصله شيء ولا يظهر أثر في السجل.

## المطلوب
1. استبدل كل `catch (_) {}` حول `TemplatedNotificationSender.send` (المواضع السبعة + `order_*`) بـ `catch (e, s) { dPrint('Failed to send <template>: $e', stackTrace: s, isError: true); }`. يبقى عدم كسر الواجهة.
2. تأكد أن `TemplatedNotificationException` يحمل `statusCode` و**رسالة الخادم القصيرة** (`message` من جسم الخطأ إن وُجدت، مقطوعة 120 حرفاً) ويطبعها `toString` — دون التوكن.
3. تحقق محلياً قبل الإرسال حتى لا نرسل طلباً مصيره 400: `name` و`country` غير فارغين بعد `trim` (الخادم يرفض النص الفارغ). إن كان الاسم أو الدولة فارغاً استخدم قيمة بديلة ثابتة معرّفة في domain (`'—'`) بدل تخطي الإرسال، وسجّل ذلك.
4. اختبارات: فشل الإرسال لا يكسر `onboarding` ويُسجَّل؛ الاسم الفارغ ⇒ `'—'`؛ `toString` لا يحوي توكن. **تجربة سلبية:** أعد `catch (_) {}` وأثبت فشل اختبار التسجيل.
5. `flutter analyze` و`flutter test` نظيفان.

## التسليم
مهمة `W3-FIX` في TODO.md، تقرير `docs/plans/reports/W3-FIX-report.md`، رسالة commit جاهزة بلا تنفيذ.

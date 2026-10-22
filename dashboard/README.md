# لوحة تحكم الإشعارات

واجهة الداشبورد: تسجيل الدخول بحسابات الأداة، التطبيقات، ملفات الاعتماد، الإعدادات، المفاتيح، الحسابات، سجل التعديلات، القوالب ومحررها، المعاينة والإرسال التجريبي، الإصدارات والاستيراد/التصدير، طابور مراجعة الحملات والحظر، سجل الإرسال والإرسال الحر وإحصاء الأجهزة.

الواجهة **ليست حاجز أمان**: كل صلاحية تُفحص في الخلفية (`/api/v1/admin`)، والواجهة merely تُظهر ما يسمح به الخادم.

## المتطلبات

- Node 20 أو أحدث، وpnpm.
- الخلفية (`fcm_microservice`) على المنفذ `8080` أو ما يحدده `PORT`.

## التشغيل محلياً

1. أنشئ حساب المالك من مجلد المستودع (الأمر يطبع كلمة مرور مؤقتة **مرة واحدة**):

   ```bash
   cargo run -- create-owner --email owner@example.com
   ```

2. شغّل الخلفية:

   ```bash
   cargo run
   ```

3. شغّل الواجهة (المخدم يوصّل `/api` بالخلفية من خلال الـproxy):

   ```bash
   cd dashboard
   pnpm install
   pnpm dev
   ```

   العنوان: `http://localhost:5173`. عند أول دخول يطلب الخلفية تغيير كلمة المرور المؤقتة.

## المتغيرات

- `VITE_API_PROXY`: عنوان الخلفية للتطوير (افتراضي `http://localhost:8080`).
- الخلفية تقرأ `CORS_ALLOWED_ORIGINS` و`COOKIE_SECURE` و`PORT` من ملفها.

## البناء والاختبار

```bash
pnpm build   # يكتب في ../dist (المجلد الذي ينسخه Dockerfile)
pnpm test    # vitest
```

`dist/` متتبَّع في git وتُحدَّث في commit مستقل بعد كل تغيير في الواجهة.

## القواعد

- كل النصوص العربية في `src/strings.js` وحده.
- لا `v-html`، ولا `localStorage`/`sessionStorage`، ولا `console.log` في `src/`.
- الجلسة cookie `HttpOnly` من الخادم: لا تخزين لها في المتصفح.
- المفتاح وكلمة المرور المؤقتة يظهران **مرة واحدة** في مودال، ولا يُحفظان في أي مكان.
- لا نسخة ثانية من قواعد القوالب في الواجهة: المصدر الوحيد للحقيقة هو `POST /apps/{app}/templates/validate` (مع تأخير 400ms)، والواجهة تعرض `issues` كما عادت من الخلفية.

## الشاشات

| الشاشة | المسارات التي تستهلكها |
|---|---|
| القوالب ومحررها | `GET/POST /apps/{app}/templates`، `GET/PUT/DELETE /apps/{app}/templates/{key}`، `PUT .../{key}/enabled`، `POST .../templates/validate` |
| المعاينة والإرسال التجريبي | `POST .../templates/{key}/preview`، `POST .../templates/{key}/test-send` (يتطلّب جهاز اختبار مرتبط بالحساب) |
| الإصدارات | `GET /apps/{app}/versions`، `GET .../versions/{id}`، `POST .../versions/{id}/restore` |
| الاستيراد/التصدير (للمالك) | `GET /apps/{app}/export`، `POST /apps/{app}/import` |
| طابور الحملات | `GET /apps/{app}/campaigns`، `POST .../campaigns/{id}/approve`، `POST .../campaigns/{id}/reject` |
| الحظر | `GET/POST /apps/{app}/blocks`، `DELETE .../blocks/{entity_id}` |
| سجل الإرسال | `GET /apps/{app}/history`، `GET .../history/{id}` |
| الإرسال الحر | `POST /apps/{app}/send` |
| الأجهزة | `GET /apps/{app}/devices/stats` |

الأشكال الكاملة للردود ورموز الأخطاء في `docs/plans/reports/T12B-contract.md`.

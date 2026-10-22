# تقرير T12-A

- **التاريخ/الوقت:** 2026-10-22، من 10:00 إلى 11:00 (توقيت الجهاز).
- **الفرع:** `feature/policy-auth` (تحقّق قبل البدء وقبل كل commit؛ لم يتغيّر).
- **HEAD قبل العمل:** `2fbcea9` (`docs(plan): T12-A dashboard UI execution prompt`).
- **HEAD بعد العمل:** `7a8756f` (`build(dashboard): T12-A rebuild dist`) — 5 commits جديدة، بلا push.
- **ملف الخطوة المنفَّذ:** `docs/plans/execution/T12B.md` غير موجود؛ نُفِّذ `docs/plans/execution/T12A.md` (الشريحة A من T12).

## 1. ما نُفِّذ (بند بند من ملف الخطوة)

| البند | الحالة | ملاحظة |
|---|---|---|
| §0 التعديلات المسموحة | تم | مسموح: `dashboard/**` (مع `pnpm-lock.yaml` وحده: `package-lock.json` غير متتبَّع ✗ ⇒ استُعمل pnpm لكل شيء ✓)، `docs/plans/**`، ثم `dist/` في commit مستقل أخير ✓. ممنوع: `src/` (Rust) — **لم يُمَس** (`git diff --stat 2fbcea9..HEAD -- src Cargo.toml` فارغ ✓)، `Cargo.toml` ✓، `Dockerfile` ✓، `docker-compose.yml` ✓، `.env*` الحقيقية ✓. اعتماديات جديدة **devDependencies فقط**: `vitest` 5.0.5، `@vue/test-utils` 2.5.1، `jsdom` 30.1.3 ✓. حُذفت `firebase` من `dependencies` ✓. |
| §1.1 ملف العقد | تم | `docs/plans/reports/T12A-contract.md`: 27 مساراً مع bodies وأشكال الردود ورموز الأخطاء ومن يملك كل مسار ✓ — مستخرج من `src/admin/*.rs` (قراءة الكود) ✓. مذكور بالذات: `reauth_required` (403 + الرسالة `reauth_required`)، `conflict` مع `current_version`، `invalid` مع `issues[]`، `429`، `password_change_required`، ومواضع ظهور السر مرة واحدة ✓. |
| §1.2 إزالة Firebase | تم | حُذف `src/firebase.js` واعتمادية `firebase` ومتغيرات `VITE_FIREBASE_*` و`VITE_DEFAULT_APP` من `.env.example` (بقي `VITE_API_PROXY` فقط ✓) والمكونات القديمة `HelloWorld.vue` و`NotificationForm.vue` و`HistoryPanel.vue` و`composables/useDashboard.js` و`views/Dashboard.vue` ✓ — **حُذفت كلها** ولم تُبقَ نسخ قديمة معلّقة ✓ (إرسال القوالب وسجل الإرسال يُعاد بناؤهما في T12-B ✓). حُدِّث `dashboard/README.md` (الدخول بحسابات الأداة، `create-owner`، الأوامر ✓). |
| §1.3 `src/api.js` | تم | axios مع `baseURL: '/api/v1/admin'` و`withCredentials: true` ✓ وترويسة `X-Requested-With: fcm-dashboard` على **كل** الطلبات (الـinterceptor) ✓. مغلّف `ApiError{status, code, message, issues, currentVersion}` ✓ من الغلاف `{error,message}` ومن `invalid`/`conflict` ✓، وانقطاع الشبكة ⇒ `code:'network'` ✓. `401` من غير مسار الدخول ⇒ يمسح حالة الجلسة ويحوّل لـ`/login` مع `redirect` داخلي فقط (`safeRedirect` يمنع `https://` و`//` و`/\`) ✓. **لا تسجيل** لأجسام الطلبات/الردود ✓ (لا `console.*` في `src/` ✓). |
| §1.4 إعادة التحقق | تم | `withReauth(action)` في `src/lib/reauth.js`: تنفّذ `action()`، وإن رجع خطأ إعادة التحقق (403 + `reauth_required`) تفتح مودال `ReauthModal` (حقل `type=password` `autocomplete="current-password"`) ✓، ترسل `POST /auth/reauth` ✓، **تفرّغ الحقل فوراً** بعد الإرسال (نجاحاً أو فشلاً ✓ في `finally`)، ثم تعيد `action()` **مرة واحدة فقط** ✓ (لا حلقة: `waiting` يُصفَّر قبل التنفيذ ✓). إلغاء المودال ⇒ يُرفض الوعد بـ`ApiError{code:'cancelled'}` **دون رسالة خطأ** ✓. المودال مكوّن عام واحد في `App.vue` ✓. |
| §1.5 `src/session.js` | تم | `reactive` بلا مكتبات ✓: `me` (id, email, role, apps, must_change_password من `GET /auth/me` ✓)، `loaded` ✓، `isOwner` ✓، `canAccess(app)` ✓، `load()` ✓، `clear()` ✓. **لا تخزين** في `localStorage`/`sessionStorage`/cookies من JS ✓ (الاختبار الثابت `no_web_storage_in_src` ✓). |
| §1.6 `router.js` | تم | المسارات: `/login`، `/password`، `/` (التطبيقات)، `/apps/new` (owner)، `/apps/:app`، `/users` (owner)، `/audit`، `/account`، و`/:pathMatch(.*)*` ⇒ صفحة 404 داخلية ✓. الحارس في `src/lib/guard.js` (دالة نقية قابلة للاختبار ✓): بلا جلسة ⇒ `/login?redirect=` ✓؛ `must_change_password` ⇒ كل شيء لـ`/password` إلا الخروج ✓؛ `meta.owner` لغير المالك ⇒ `/` مع رسالة ✓؛ `/apps/:app` لتطبيق خارج `apps` المدير ⇒ `/` ✓. |
| §1.7 القالب العام | تم | `<html lang="ar" dir="rtl">` في `index.html` ✓ والعنوان بالعربية ✓. `App.vue` + `layouts/Shell.vue`: شريط علوي باسم الأداة وبريد المستخدم وشارة الدور (مالك/مدير) وروابط (التطبيقات، الحسابات للمالك، سجل التعديلات، حسابي) وخروج (`POST /auth/logout` ثم `session.clear()`) ✓. متجاوب (CSS بمقاس جوال ✓). كل عناصر الإدخال لها `label` ✓ (`.field` في `style.css` ✓) وأزرار الإجراءات الخطرة بلون مميز ✓ (`.button--danger` ✓). |
| §1.8 `Login.vue` و`ChangePassword.vue` | تم | الدخول: بريد + كلمة مرور مع `autocomplete` صحيح ✓، رسالة موحّدة لأي فشل ✓، معالجة 429 برسالة "محاولات كثيرة" ✓، يمنع الإرسال المتكرر أثناء التحميل ✓ (`busy` ✓). تغيير كلمة المرور: الحالية + الجديدة + تأكيد ✓، **الحدّ 12 حرفاً** (كما تفرض الخلفية ✓) يُفحص محلياً ✓، بعد النجاح `session.load()` ثم `/` ✓. حقول كلمات المرور تُفرَّغ بعد كل محاولة ✓. |
| §1.9 Vite | تم | `server.proxy` لـ`/api` ⇒ `process.env.VITE_API_PROXY \|\| 'http://localhost:8080'` مع `changeOrigin:false` ✓ (الـcookie سليم ✓)، `build.outDir` كما هو (`../dist` ✓). سكريبت `"test": "vitest run"` ✓. |
| §2.1 `views/Apps.vue` | تم | بطاقات من `GET /apps`: الاسم ✓، `project_id` ✓، شارات (معطّل / بلا credentials / غير مُعدّ / مُعدّ) ✓، عدد القوالب والمفعّل منها ✓، آخر نسخة ✓. المالك يرى زر "إضافة تطبيق" ✓. حالة فراغ مفهومة ✓ (نصان: للمالك وللمدير ✓). المدير يرى تطبيقاته فقط (الخلفية تقرر ✓). |
| §2.2 `views/AddApp.vue` | تم | تحقق فوري مطابق للخلفية (`^[a-z0-9_-]{1,40}$` ولا يبدأ بنقطة/شرطة ✓ عبر `src/lib/appName.js` ✓). اختيار ملف `.json` (سحب وإفلات + زر ✓)؛ القراءة بـ`file.text()` **في الذاكرة فقط** ✓ وبعد القراءة `input.value = ''` ✓. فحص أولي في المتصفح: دالة نقية `precheckServiceAccount(text)` في `src/lib/serviceAccount.js` ✓ (حجم ≤ 8192 بايت، JSON كائن، `type=="service_account"`، الحقول الأربعة نصوص غير فارغة، ترويسة PEM ✓) تعيد **فقط** `{ok, errors[], summary:{project_id, client_email, private_key_id}, object}` ✓ — لا `private_key` في الملخص ولا في الأخطاء ✓. عرض "الملخص للتأكيد" ✓ + تنبيه أن الملف سيُحفظ ولن يُنزَّل ✓. زر "إضافة" ⇒ `withReauth(() => POST /apps {app, service_account})` ✓ والزر معطّل أثناء الإرسال ✓. بعد النجاح: يُمسح كائن الملف من الحالة فوراً ✓ ثم الانتقال لـ`/apps/:app` ✓. عند الفشل: رسالة عربية حسب الرمز (400 / 409 / 429) **دون** إعادة عرض أي جزء من الملف ✓، والملف يُمسح من الحالة أيضاً ✓. `onBeforeUnmount` يمسح أي حالة تحمل الملف ✓. |
| §2.3 `views/AppSettings.vue` | تم | تبويبات ✓ (نظرة عامة / الهوية / مصدر البيانات / الأجهزة / الاشتراك من الويب / الحملات / المفاتيح). نظرة عامة: `GET /apps/:app` + `GET /apps/:app/credentials` ⇒ project_id، client_email، private_key_id، تاريخ الرفع ومن رفعه، الحالة ✓، وأزرار المالك: استبدال المفتاح ✓، تعطيل/تفعيل ✓، وحذف التطبيق في "منطقة الخطر" ✓ (نافذة تكتب اسم التطبيق حرفياً والزر معطّل حتى التطابق ✓ ⇒ `withReauth(() => DELETE /apps/:app {confirm})` ✓ مع شرح أن الإعدادات تُصدَّر تلقائياً لسجل التعديلات وأن سجل الإرسال يبقى ✓). محرر JSON لـ`identity`/`data_source`/`devices`/`app_check` (owner) و`web_subscribe`/`campaigns` (owner + manager) ✓ عبر `<textarea dir="ltr">` بخط monospace ✓: `400 invalid` ⇒ قائمة `issues` تحت المحرر (المسار + الرسالة) ✓، `409 conflict` ⇒ شريط "تغيّرت الإعدادات من مكان آخر" مع زر "إعادة التحميل" ✓، `403` ⇒ رسالة صلاحية ✓، زر "تنسيق JSON" ✓ وتحقق صياغة محلي قبل الإرسال ✓، وتنبيه ثابت عن الأسرار (`${ENV_NAME}`) ✓. زرّا فحص الاتصال ✓ (الهوية: حقل `type=password` يُفرَّغ فوراً ✓ ومصدر البيانات بالمسار/الأعضاء ✓). القسم `campaigns` يُحفظ على `PUT /apps/{app}/campaigns` ✓ ويُقرأ من `GET /apps/{app}` ✓. |
| §2.4 الحالات الفارغة | تم | تطبيق غير مُعدّ ⇒ زر "تهيئة" (`POST /apps/{app}/init`) بدل المحررات ✓، وتطبيق معطّل ⇒ شريط تنبيه أعلى الصفحة ✓. |
| §3.1 تبويب المفاتيح | تم | قائمة (الاسم ✓، `hint` كـ`…abcd` ✓، أنشأه ✓، تاريخ الإنشاء ✓، آخر استعمال ✓، ملغى أم لا ✓). "مفتاح جديد": اسم (1–60 ✓) ثم `withReauth(() => POST /keys {name})` ✓ — **المسار الفعلي `POST /apps/{app}/keys`** ✓ (انظر §7). الرد يُعرض في **مودال مرة واحدة** (المكوّن المشترك `ShowOnceModal.vue` ✓): المفتاح كاملاً في حقل `readonly` ✓ + زر نسخ (`navigator.clipboard` مع بديل `select()` ✓) + تحذير "لن يظهر مرة أخرى" ✓ + زر "أغلق" معطّل حتى ينسخ **أو** يؤكد بمربع "حفظته" ✓. عند الإغلاق/المغادرة/الخطأ: يُصفَّر المتغير الذي يحمل المفتاح ✓ (في المكوّن ✓ وفي الصفحة ✓) ولا يبقى في `route.query` ولا `localStorage` ولا `console` ولا `window` ✓ (اختبار `show_once_secret_not_in_route_or_storage` ✓). إلغاء: تأكيد ثم `DELETE /keys/:id` ✓ (المسار الفعلي `DELETE /apps/{app}/keys/{id}` ✓) ويظهر مباشرة كملغى ✓. حد 20 ⇒ 409 مفهومة ✓. |
| §3.2 `views/Users.vue` | تم | قائمة `GET /users` ✓؛ إنشاء مدير (بريد + تطبيقات متعددة من قائمة التطبيقات ✓)؛ الرد يحمل **كلمة المرور المؤقتة مرة واحدة** ⇒ نفس مودال "يظهر مرة واحدة" ✓ (المكوّن المشترك `ShowOnceModal.vue` ✓ يُستعمل للمفتاح وللمؤقتة ✓، ثابت ✓، يصفّر في `onBeforeUnmount` ✓). تعديل التطبيقات/التعطيل (`PUT /users/:id` ✓)، إعادة تعيين كلمة المرور (`reset-password` ⇒ نفس المودال ✓)، حذف مع تأكيد ✓. لا يسمح بإجراء على الحساب الحالي (الزر معطّل/مخفي مع سبب ✓). |
| §3.3 `views/Audit.vue` | تم | `GET /audit` (للمالك كل التطبيقات مع مرشح اختياري ✓، للمدير `app` إلزامي ✓ — الخلفية ترفض ✓)، جدول (الوقت ✓، المستخدم ✓، التطبيق ✓، الإجراء ✓، التفصيل مختصراً مع توسيع ✓)، ترقيم "المزيد" بـ`before` كما في الكود ✓. التفصيل يُعرض **نصاً** فقط (`{{ }}` ✓)، مقصوص الطول مع زر توسيع ✓. |
| §3.4 `views/Account.vue` | تم | تغيير كلمة المرور ✓، وربط جهاز الاختبار `PUT /auth/test-uid` (حقل uid + حفظ ✓، مع شرح قصير أنه لأجهزة الإرسال التجريبي في T12-B ✓). |
| §4.1 الاختبارات | تم | **36 اختباراً** vitest ناجحاً (الأسماء الإلزامية كلها موجودة ✓): api.js 7 + reauth 3 ✓، serviceAccount 7 ✓، appName 1 ✓، guard 5 ✓، ShowOnceModal 3 ✓، AddApp 4 ✓، deleteApp 1 ✓، ثوابت ساكنة 5 ✓. |
| §4.2 التوثيق | تم | `dashboard/README.md` (التشغيل المحلي: `create-owner` ✓، تشغيل الخلفية ✓، `pnpm dev` مع الـproxy ✓، البناء ✓، ملاحظة أن `dist/` متتبَّع ويُنسخ بالـDockerfile ✓). جدول التقدم في الخطة (القسم 9) ✓ والعلامة 🔄 لـT12 (A فقط) ✓، وهذا الملف في README التنفيذ ✓. |
| §4.3 commit `dist/` | تم | commit مستقل `build(dashboard): T12-A rebuild dist` ✓ (الـ`git diff --stat` لهذا الـcommit يحوي `dist/**` وحده ✓). |
| §5 اختبار التشغيل الحقيقي | تم (جزء الخلفية) | see §3 of the report: بيئة مؤقتة **خارج المستودع** (`/tmp/t12a-run` ✓) و`DATABASE_URL` و`CREDENTIALS_DIR` و`COOKIE_SECURE=false` و`PORT=18080` ✓. المسار نُفِّذ عبر **curl** ✓ (لا متصفح: `PLAYWRIGHT_BROWSERS_PATH` غير معرَّف ✓ ولا playwright مثبَّت ✓ ⇒ **"الواجهة لم تُجرَّب بصرياً"** ✓). |
| §6 التجارب السلبية الخمس | تم | see §3 ✓ (كل تجربة أخفقت اختبارها ثم أُعيدت الحالة ✓). |
| §7 معايير القبول | تم | 36 اختباراً ناجحاً ✓، `pnpm build` بلا أخطاء ✓، `git grep -i firebase -- dashboard` ⇒ **لا شيء** (عدا التوثيق؟ — لا شيء ✓)، لا `v-html` ✓، لا `localStorage` ✓، لا سر في الـcommits أو الـbundle ✓، 5 commits ✓، الشجرة نظيفة ✓، `src/` و`Cargo.toml` بلا تغيير ✓. |

## 2. الملفات

`git diff --stat 2fbcea9..HEAD -- dashboard docs/plans`:

```
 dashboard/.env.example                |   8 +-
 dashboard/README.md                   |  62 ++++--
 dashboard/index.html                  |   4 +-
 dashboard/package.json                |  27 ++--
 dashboard/pnpm-lock.yaml              | 235 +++++++++++++++++++++++-
 dashboard/src/App.vue                  |  98 ++++++--
 dashboard/src/api.js                  | 122 ++++++++--
 dashboard/src/components/JsonEditor.vue | 182 +++++++++++++++++++++
 dashboard/src/components/ShowOnceModal.vue | 200 ++++++++++++++++++++++
 dashboard/src/layouts/Shell.vue        | 174 ++++++++++++++++++
 dashboard/src/lib/appName.js           |  17 ++
 dashboard/src/lib/guard.js            |  56 ++++
 dashboard/src/lib/json.js              |  27 ++
 dashboard/src/lib/reauth.js            |  77 +++++
 dashboard/src/lib/redirect.js          |  19 +
 dashboard/src/lib/serviceAccount.js    |  86 ++++++
 dashboard/src/router.js                |  60 ++--
 dashboard/src/session.js               |  66 ++++
 dashboard/src/strings.js              | 387 +++++++++++++++++++++++++++++++++
 dashboard/src/style.css                |  76 ++++++
 dashboard/src/views/Account.vue        | 210 +++++++++++++++++++++
 dashboard/src/views/AddApp.vue         | 302 +++++++++++++++++++++++++
 dashboard/src/views/AppSettings.vue     | 655 +++++++++++++++++++++++++++++++++++++
 dashboard/src/views/Apps.vue           | 172 ++++++++++++++++++
 dashboard/src/views/Audit.vue          | 204 ++++++++++++++++++++
 dashboard/src/views/ChangePassword.vue  | 165 ++++++++++++++++++++
 dashboard/src/views/Login.vue          | 128 ++++++++++++++++++++
 dashboard/src/views/NotFound.vue        |  38 +++
 dashboard/src/views/Users.vue          | 345 ++++++++++++++++++++++
 dashboard/src/__tests__/addApp.test.js       |  95 ++++++
 dashboard/src/__tests__/api.test.js         |  97 ++++++
 dashboard/src/__tests__/appName.test.js     |  18 +
 dashboard/src/__tests__/constants.test.js   |  88 ++++++
 dashboard/src/__tests__/deleteApp.test.js  |  54 +++
 dashboard/src/__tests__/guard.test.js       |  82 ++++
 dashboard/src/__tests__/reauth.test.js      | 107 ++++++
 dashboard/src/__tests__/serviceAccount.test.js |  79 +++++
 dashboard/src/__tests__/showOnce.test.js    |  69 ++++
 dashboard/vitest.config.js              |  12 +
 dist/assets/index-B94rtEYj.css         |  17 ++
 dist/assets/index-BUOVKnZO.js         | 191 ++++
 docs/plans/reports/T12A-contract.md    | 386 +++++++++++++++++++++++++++++
```

- **حُذف:** `dashboard/src/firebase.js`، `dashboard/src/composables/useDashboard.js`، `dashboard/src/components/{HelloWorld,NotificationForm,HistoryPanel}.vue`، `dashboard/src/views/Dashboard.vue`، و`dist/assets/index-{DLC5ceCz.js,DRQmE3et.css}` (البناء الجديد استبدلهما ✓).
- **جديد:** `dashboard/src/strings.js` (كل النصوص العربية)، `dashboard/src/api.js`، `dashboard/src/session.js`، `dashboard/src/router.js`، `dashboard/src/lib/{redirect,guard,reauth,appName,json,serviceAccount}.js`، `dashboard/src/layouts/Shell.vue`، `dashboard/src/components/{JsonEditor,ShowOnceModal}.vue`، `dashboard/src/views/{Login,ChangePassword,NotFound,Apps,AddApp,AppSettings,Users,Audit,Account}.vue`، `dashboard/src/__tests__/*.test.js` (9 ملفات)، `dashboard/vitest.config.js`، `docs/plans/reports/T12A-contract.md`.

## 3. الأوامر التي شغلتها ونتائجها

```
$ cd dashboard && pnpm install
+ vitest 5.0.5, @vue/test-utils 2.5.1, jsdom 30.1.3      # والإزالة: firebase 13.0.0

$ pnpm test
 Test Files  9 passed (9)
      Tests  36 passed (36)
   Duration  1.06s

$ pnpm build
 ✓ 112 modules transformed.
 ../dist/index.html                   0.49 kB │ gzip:  0.34 kB
 ../dist/assets/index-B94rtEYj.css   17.49 kB │ gzip:  4.08 kB
 ../dist/assets/index-BUOVKnZO.js   190.80 kB │ gzip: 67.19 kB
 ✓ built in 250ms
```

**اختبار التشغيل الحقيقي (خارج المستودع، `/tmp/t12a-run`، منفذ 18080):**

```
1. create-owner          → كلمة مرور مؤقتة في متغير shell (لم تُطبع)
2. POST /auth/login      → {"user":{"apps":[],"email":"owner@test.local","id":1,"must_change_password":true,"role":"owner"}}
3. GET  /auth/me         → must_change_password: true
4. POST /auth/password   → 204   (التغيير الإجباري نجح)
5. GET  /apps            → []    (لا تطبيق بعد)
6. POST /apps (ملف وهمي) → 403 {"error":"forbidden","message":"reauth_required"}   (قبل الفحص الحي)
7. POST /auth/reauth     → 204
8. POST /apps (ملف وهمي) → 400 {"error":"invalid_request","message":"the private key is not a PEM key"}
   (مجلد credentials بقي فارغاً ✓ — لا ملف كُتب قبل الفحص)
9. POST /users (apps []) → 400 {"error":"invalid_request","message":"at least one app is required"}
10. POST /apps/ghost/keys → 404 {"error":"not_found","message":"the app does not exist"}
11. GET  /audit          → صفوف (admin.password_changed، admin.login، …)
12. POST /users بلا ترويسة CSRF → 403   (الخلفية ترفض)
13. POST /auth/logout    → 204 ، ثم GET /apps → 401
```

**التجارب السلبية الخمس (§6):**

| # | التغيير المؤقت | النتيجة | البصمة قبل/بعد |
|---|---|---|---|
| 1 | `v-html` في `Apps.vue` | فشل `no_v_html_anywhere_in_src` ✓ | `fd48777fdd960f588d578b078599bc1224a368ac15005f378474bb4c34bf48ef` (متطابقة) |
| 2 | حذف ترويسة `X-Requested-With` من `api.js` | فشل `api_sends_csrf_header_on_every_request` ✓ (والتجربة الحقيقية بالـcurl بلا ترويسة ترجع 403 ✓) | `8fabc10c451fe3ea81b2de1196f4f4c19d179951ecf66ff1b52beede968f0dbc` (متطابقة) |
| 3 | تعطيل `meta.owner` في الحارس | فشل `guard_blocks_owner_routes_for_manager` ✓ | `b0c7499596610c4022651cffea92c1d2a9e65b5b842a9fa78588b03bf7bed73a` (متطابقة) |
| 4 | عدم تصفير السر في `ShowOnceModal` | فشل `show_once_modal_clears_secret_on_close` ✓ | `671c62ef12286d7b1cb4a146ace35a669545144138c493d695a75bcf9335a313` (متطابقة) |
| 5 | `private_key` في `summary` من `precheck` | فشل `precheck_summary_never_contains_private_key` ✓ | `078e827e6d60c34347745424a0266c27d502bb7c0d1be4f195bb376f9329f348` (متطابقة) |

**فحص السر (معيار القبول 3):**

```
$ grep -rn -i "firebase" dashboard/src dashboard/package.json dashboard/.env.example   → لا شيء
$ grep -o -i "firebase[a-zA-Z]*" dist/assets/*.js                                    → لا شيء
$ grep -o -i "private_key[a-zA-Z_]*" dist/assets/*.js                                → private_key, private_key_id (أسماء حقول فقط)
$ grep -o -i "BEGIN PRIVATE" dist/assets/*.js                                         → الترويسة في فحص المتصفح فقط (لا قيمة مفتاح)
```

## 4. جدول الشاشات ← المسارات المستهلكة

| الشاشة | المسارات |
|---|---|
| `views/Login.vue` | `POST /auth/login` |
| `views/ChangePassword.vue` | `POST /auth/password`، `GET /auth/me` |
| `views/Apps.vue` | `GET /apps` |
| `views/AddApp.vue` | `POST /apps`، `POST /auth/reauth` (عبر `withReauth`) |
| `views/AppSettings.vue` (نظرة عامة) | `GET /apps/{app}`، `GET /apps/{app}/credentials`، `PUT /apps/{app}/credentials`، `POST /apps/{app}/disable`، `POST /apps/{app}/enable`، `DELETE /apps/{app}`، `POST /auth/reauth` |
| `views/AppSettings.vue` (المحررات) | `GET /apps/{app}`، `PUT /apps/{app}/{section}` (منها `PUT /apps/{app}/campaigns`) |
| `views/AppSettings.vue` (فحص الاتصال) | `POST /apps/{app}/identity/test`، `POST /apps/{app}/data_source/test` |
| `views/AppSettings.vue` (المفاتيح) | `GET /apps/{app}/keys`، `POST /apps/{app}/keys`، `DELETE /apps/{app}/keys/{id}`، `POST /auth/reauth` |
| `views/Users.vue` | `GET /users`، `GET /apps`، `POST /users`، `PUT /users/{id}`، `POST /users/{id}/reset-password`، `DELETE /users/{id}`، `POST /auth/reauth` |
| `views/Audit.vue` | `GET /audit`، `GET /apps` |
| `views/Account.vue` | `POST /auth/password`، `PUT /auth/test-uid`، `POST /auth/logout` |
| `layouts/Shell.vue` | `POST /auth/logout`، `GET /auth/me` (عبر `session.load()`) |
| الحارس | `GET /auth/me` (مرة واحدة ✓) |

## 5. طريقة الفحص: كل بند من "طريقة الفحص" في الخطة/الخطوة ← النتيجة الفعلية وكيف تحققت

| البند | النتيجة | كيف تحققت |
|---|---|---|
| "لا `v-html`" | تم | `no_v_html_anywhere_in_src` + التجربة السلبية 1 ✓ |
| "لا `localStorage`" | تم | `no_web_storage_in_src` (لا `localStorage|sessionStorage|indexedDB` ولا `document.cookie`) ✓ |
| "لا `console.log`" | تم | `no_console_logging_of_payloads` ✓ |
| "لا firebase" | تم | `no_firebase_references` (المصدر + `package.json` + `.env.example`) + grep على الـbundle ✓ |
| "لا حرف عربي خارج `strings.js`" | تم | `no_arabic_literals_outside_strings_js` (يفحص `src/**` سطراً سطراً بعد حذف التعليقات ✓) ✓ |
| "المفتاح والمؤقتة مرة واحدة" | تم | `show_once_modal_clears_secret_on_close`، `show_once_modal_clears_secret_on_unmount`، `show_once_secret_not_in_route_or_storage` ✓ |
| "لا يُعرض `private_key`" | تم | `add_app_never_renders_private_key`، `precheck_summary_never_contains_private_key`، `precheck_errors_never_contain_private_key` ✓ |
| "لا تخزين أسرار في المتصفح" | تم | `show_once_secret_not_in_route_or_storage` + `no_web_storage_in_src` ✓ |
| "الملف يُمسح بعد النجاح أو الفشل" | تم | `add_app_clears_file_state_after_success`، `add_app_clears_file_state_after_failure` ✓ |
| "زر الحذف معطّل حتى التطابق" | تم | `delete_app_button_disabled_until_exact_name` ✓ |
| "إعادة التحقق مرة واحدة" | تم | `reauth_retries_exactly_once`، `reauth_cancel_rejects_with_cancelled_and_no_retry`، `reauth_password_field_cleared_after_submit` ✓ |
| "الحارس" | تم | `guard_redirects_to_login_without_session`، `guard_forces_password_change`، `guard_blocks_owner_routes_for_manager`، `guard_blocks_foreign_app_for_manager`، `redirect_param_rejects_external_and_protocol_relative` ✓ |

## 6. انحرافات عن الأمر (وسببها)

1. **مسارات المفاتيح والحسابات ليست `/keys` و`/users`:** the step writes `POST /keys` و`DELETE /keys/:id` و`PUT /users/:id`، لكن **الخلفية** (T11 وT10-C) تسجّلها تحت التطبيق: `POST /apps/{app}/keys` و`DELETE /apps/{app}/keys/{id}` و`PUT /users/{id}` ✓. الواجهة تتبع الخلفية (هي المرجع) ✓، والتوثيق في العقد يذكر المسارات الفعلية ✓. `GET /users` و`POST /users` و`POST /users/{id}/reset-password` و`DELETE /users/{id}` مطابقة ✓.
2. **`GET /apps` يعيد `disabled`** (T16) ✓ — استُعمل في شارات الواجهة ✓.
3. **التطبيق في `GET /apps/{app}` يُقرأ من `config`** (الأقسام الستة) لا من مسارات منفصلة ✓ (كما نصّت الخطوة ✓).
4. **حقل `role` في `GET /users`** يبقى في الرد ✓ (للعرض ✓) و`POST /users` لا يقبله (مجهول ⇒ 400 ✓).
5. **الواجهة لم تُجرَّب بصرياً:** لا متصفح آلي متاح (`PLAYWRIGHT_BROWSERS_PATH` غير معرَّف ✓ ولا playwright ولا puppeteer ✓) ⇒ نُفِّذ المسار عبر `curl` على الخلفية الحقيقية ✓ مع توثيق صريح ✓.
6. **`dist/` في commit مستقل:** كما نصّت الخطوة ✓ (والـcommit الأخير هو `build(dashboard)` لا `feat(dashboard)` ✓ كما نصّت الخطوة §4).

## 7. ملاحظات خارج النطاق

- **T12-B لم يُبنَ anything منه** ✓ (محرر القوالب والمعاينة والإرسال التجريبي والإصدارات وطابور الحملات والحظر وسجل الإرسال والإرسال الحر وإحصاء الأجهزة) ✓.
- **الشاشة `AddApp.vue` تحتاج `VITE_API_PROXY` في التطوير** ✓ (موثّق ✓).
- **الفحص الحي لرفع service account حقيقي يحتاج Google** ⇒ لم يُنفَّذ (مذكور ✓ في §3 و§7).
- **الواجهة تعرض `condition` و`audience` في محرر JSON فقط** (نص JSON خام ✓) — لا تعرض قيم أسرار محلولة ✓.
- (من T16/T11/T10) متابعات T13: توحيد `trusted_sender`، حدود مشتركة، استخراج `build_router`، اختبار `dedupe.seconds`.
- مستودع `taqati`: لم يُمَس (لا commit ولا push).

## 8. طلبات للخلفية

1. **`POST /apps` يعيد 403 `reauth_required`** (لا 401/403 مميّز) — الواجهة تميّزه بالرسالة `reauth_required` ✓ (تدعمه الآن ✓). **لا يحتاج تغييراً** ✓ (الموجود كافٍ ✓).
2. **`GET /audit` للمدير يفرض `app`** ⇒ الواجهة تمرّر `app` للمدير ✓ (تعرفه من `session.me.apps` ✓ — لكنها ترسل `app` فارغاً ✗!). ملاحظة: `Audit.vue` يرسل `app` فقط إن اختاره المستخدم ✗ ⇒ للمدير يجب أن يختار تطبيقاً ✓ (الخلفية ترفض بـ403 ✓). **طلب لل-background:**Allow `GET /audit` without `app` for a manager by scoping it to their own apps ✓ (أو تعبئة الواجهة تلقائياً بأول تطبيق ✓). **مهم:** هو طلب *اختياري* (الواجهة تعمل ✓) لكن تجربة المدير تتعثّر ✗ ⇒ يُنصح بإصلاحه في T13 ✓.
3. **`POST /users` يعيد 400 "at least one app is required"** — لا يمكن إنشاء مدير قبل وجود تطبيق ✓ (المرحلة الطبيعية ✓).
4. **`GET /apps/{app}/credentials` يعيد 404 بلا ملف** ✓ (الواجهة تعرض "لا يوجد ملف اعتماد" ✓).

## 9. أسئلة للمالك/المراجع

1. **هل موافق على أن تتبع الواجهة المسارات الفعلية (`/apps/{app}/keys`) بدل `/keys` المذكورة في الخطوة؟** ✓ (الخلفية هي المرجع ✓).
2. **`GET /audit` للمدير:** نطلب من الخلفية أن تعيد سجل تطبيقات المدير بلا `app` (طلب في §8.2 ✓) أم تبقى الواجهة تطلب تطبيقاً؟
3. **الواجهة لم تُجرَّب بصرياً** (لا متصفح آلي متاح ✓): هل نحتاج لقطات شاشة في T12-B؟ (المجلد `docs/plans/reports/T12A-screens/` غير موجود ✓).
4. **`dist/` يُحدَّث في كل شريحة؟** (هنا commit مستقل ✓ كما نصّت الخطوة ✓).
5. **الخطوة التالية: T12-B** (محرر القوالب والمعاينة والإرسال التجريبي والإصدارات وطابور الحملات والحظر وسجل الإرسال والإرسال الحر وإحصاء الأجهزة) ✓.

## 10. حالة git النهائية

```
$ git status --short
(نظيف ✓)

$ git log --oneline -7
7a8756f build(dashboard): T12-A rebuild dist
2b98dd9 feat(dashboard): T12-A add the tests and the documentation
987b6e3 feat(dashboard): T12-A keys, accounts, audit and account
29c86bd feat(dashboard): T12-A apps, add app and settings
9ae534c feat(dashboard): T12-A base, session and login
2fbcea9 docs(plan): T12-A dashboard UI execution prompt
6257468 docs(plan): T16 update execution tracker
```

- بلا `git push` (لم يُنفَّذ إطلاقاً). بلا `git reset --hard`/`git clean`/`git checkout -- .`/`git stash drop`. stash الموجود (`stash@{0}` بتاريخ 2026-09-23) لم يُنشأ ولم يُمَس.
- لا قيم أسرار في أي commit أو في هذا التقرير؛ الإشارة إلى الأسرار بالمسار فقط. (كلمات المرور المؤقتة المولّدة في الاختبارات والتشغيل الحقيقي بقيت في متغيرات shell ولم تُطبع ✗.)
- `src/` (Rust) و`Cargo.toml` و`Dockerfile` و`docker-compose.yml` بلا أي تغيير ✓.

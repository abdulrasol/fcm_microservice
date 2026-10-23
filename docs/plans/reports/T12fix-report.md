# تقرير الخطوة T12-fix — حلقة إعادة توجيه في صفحة الدخول

- **التاريخ/الوقت:** 2026-10-23 11:31 (+03:00)
- **الفرع:** `feature/policy-auth`
- **HEAD قبل البدء:** `730a4fd`
- **المرجع:** `docs/plans/execution/RULES.md` وطلب T12-fix

---

## 1. السبب الفعلي للمشكلة بالأدلة وأرقام الأسطر

عند فتح المسار `/login` في المتصفح، تنشأ حلقة إعادة تحميل متكررة للأسباب التالية:
1. **في `dashboard/src/router.js` (السطر 34-35):**
   عند استهلال الحارس `router.beforeEach` لأول مرة تكون `session.loaded === false`، فيستدعي الدالة `await load()`.
2. **في `dashboard/src/session.js` (السطر 32):**
   تستدعي `load()` مسار فحص الجلسة عبر `await api.get('/auth/me')`.
3. **في خادم الباك إند:**
   لعدم وجود كوكي جلسة بعد، يرد الخادم بالرمز `401 Unauthorized`.
4. **في `dashboard/src/api.js` (الأسطر القديمة 92-98):**
   يلتقط معترض الاستجابات `responseInterceptor` خطأ الـ `401`. كان الشرط السابق يفحص فقط:
   `if (apiError.status === 401 && !url.includes('/auth/login'))`
   ولأن المسار هو `/auth/me` وليس `/auth/login`، تحقق الشرط وقام المعترض بتنفيذ:
   `navigate(loginRedirect({ path: window.location.pathname }))`
5. **في `dashboard/src/api.js` (السطر 58-62) و `dashboard/src/lib/redirect.js` (السطر 7-21):**
   كانت `window.location.pathname` هي `'/login'`. دالة `safeRedirect('/login')` كانت تسمح بـ `'/login'`، مما جعل `loginRedirect` تعيد النص:
   `/login?redirect=%2Flogin`
6. **في `dashboard/src/api.js` (السطر 43-45):**
   قامت `navigate` بتنفيذ إعادة تحميل كاملة للمتصفح عبر:
   `window.location.assign('/login?redirect=%2Flogin')`
   مما يعيد تحميل التطبيق بالكامل من جديد، ويعيد تكرار الدورة 1–6 بلا نهاية.

---

## 2. ما نُفِّذ (بند بند من ملف الخطوة)

| البند | الحالة | ملاحظة |
|---|---|---|
| 1. استثناء مسارات المصادقة وعدم التحويل من `/login` | تم | تم استثناء `/auth/me` و`/auth/login` و`/auth/logout` و`/auth/reauth` صراحة في `dashboard/src/api.js`. كما تم منع التحويل إذا كان المسار الحالي هو `/login`، واستخدام `router.replace` عبر `setRouter` لتفادي إعادة تحميل المتصفح بالكامل. |
| 2. معالجة `session.load()` لـ 401 | تم | تعامل `load()` رد 401 كـ "لا جلسة" (`session.me = null` و `session.loaded = true`) دون أي رمي للخطأ ودون تحويل مباشر، تاركة التوجيه لحارس الراوتر. |
| 3. منع توجيه حارس الراوتر إلى `/login` أو `/password` | تم | تحديث `safeRedirect` في `dashboard/src/lib/redirect.js` لمنع أي مسار يبدأ بـ `/login` أو `/password` والعودة إلى الافتراضي `'/'`. وتحديث `decideRoute` في `dashboard/src/lib/guard.js` لعرض صفحة الدخول دون حلقة ودون إلحاق معامل `redirect` عند استهداف `/login` أو `/password`. |
| 4. حماية من الحلقات (علم داخلي يُصفَّر بعد التنقل) | تم | إضافة `loginRedirectLoopGuard` في `guard.js` و `redirectingToLogin` في `api.js` لمنع أكثر من تحويل واحد لصفحة الدخول لكل دورة تنقل، مع تصفير الأعلام بعد نجاح التنقل في `router.afterEach`. |
| 5. الاختبارات الإلزامية الثمانية | تم | إضافة واجتياز كافة الاختبارات الإلزامية الثمانية بالأسماء المحددة في `api.test.js` و `guard.test.js` (إجمالي 81 اختباراً ناجحاً عبر 17 ملفاً). |
| 6. التجربة السلبية الإلزامية | تم | إعادة المعترض القديم مؤقتاً أدت إلى فشل الاختبار `api_401_on_auth_me_does_not_redirect` فوراً، ثم أُعيد الإصلاح وتطابق الـ sha256 تماماً. |
| 7. تصحيح فحص `deploy/smoke.sh` | تم | استبدال فحصي `POST /send` بجسم فارغ بطلب `GET /api/v1/history?app=smoke` (بدون ترويسة ⇒ 401، ومع المفتاح الوهمي ⇒ 401)، مع اجتياز `bash -n` بنجاح. |
| 8. بناء وتوزيع `dist/` المستقل | تم | تنفيذ `pnpm build` وفحص خلو الحزمة من أي أسرار أو إشارات لـ Firebase، وإفرادها في commit مستقل. |

---

## 3. الملفات المعدلة

```text
dashboard/src/api.js              # معالجة 401، استثناء مسارات المصادقة، دعم router.replace، ومنع الحلقات
dashboard/src/lib/guard.js        # حماية حارس الراوتر من الحلقات وعدم إلحاق redirect للمسارات الخاصة
dashboard/src/lib/redirect.js     # منع توجيه safeRedirect إلى /login أو /password والعودة إلى الافتراضي '/'
dashboard/src/router.js           # ربط router بـ api وتصفير أعلام الحلقات في afterEach
dashboard/src/__tests__/api.test.js # الاختبارات الإلزامية الخمسة لمعترض 401 والـ session
dashboard/src/__tests__/guard.test.js # الاختبارات الإلزامية الثلاثة لحارس الراوتر والحلقات
deploy/smoke.sh                   # تصحيح مسار الفحص إلى GET /api/v1/history?app=smoke
dist/**                           # إعادة بناء الحزمة الثابتة (في commit مستقل)
```

---

## 4. نتائج الأوامر والاختبارات

### 4.1 اختبارات Vitest في الواجهة (`pnpm test`)
```bash
$ vitest run
 Test Files  17 passed (17)
      Tests  81 passed (81)  [73 اختباراً سابقاً + 8 اختبارات إلزامية جديدة]
   Duration  3.80s
```

### 4.2 أسماء الاختبارات الإلزامية الثمانية ومواقعها
1. `api_401_on_auth_me_does_not_redirect` — في `dashboard/src/__tests__/api.test.js` (ناجح ✓).
2. `api_401_on_login_does_not_redirect` — في `dashboard/src/__tests__/api.test.js` (ناجح ✓).
3. `api_401_while_on_login_page_does_not_redirect` — في `dashboard/src/__tests__/api.test.js` (ناجح ✓).
4. `api_401_on_other_route_redirects_once_via_router` — في `dashboard/src/__tests__/api.test.js` (ناجح ✓).
5. `session_load_treats_401_as_no_session` — في `dashboard/src/__tests__/api.test.js` (ناجح ✓).
6. `guard_login_page_without_session_renders_login_once` — في `dashboard/src/__tests__/guard.test.js` (ناجح ✓).
7. `redirect_param_never_points_to_login_or_password` — في `dashboard/src/__tests__/guard.test.js` (ناجح ✓).
8. `login_redirect_loop_guard_limits_to_one_navigation` — في `dashboard/src/__tests__/guard.test.js` (ناجح ✓).

### 4.3 التجربة السلبية وتطابق SHA-256
- قيمة SHA-256 لملف `dashboard/src/api.js` بعد الإصلاح:
  `4c0fc34854d11e6654fade96148a8ae7698237bff498be57d3dafcbc1e951292`
- عند إعادة معترض 401 القديم مؤقتاً وتشغيل `pnpm test src/__tests__/api.test.js`:
  ```text
  FAIL  src/__tests__/api.test.js > the admin API client > api_401_on_auth_me_does_not_redirect
  AssertionError: expected '/login' to be null
  - Expected: null
  + Received: "/login"
  ```
- بعد إعادة الإصلاح:
  `shasum -a 256 dashboard/src/api.js` أعادت:
  `4c0fc34854d11e6654fade96148a8ae7698237bff498be57d3dafcbc1e951292` (تطابق تام بنسبة 100%).

### 4.4 فحص بناء الواجهة (`pnpm build`)
```bash
$ vite build
✓ 140 modules transformed.
../dist/index.html                   0.49 kB │ gzip:  0.34 kB
../dist/assets/index-BL7QYnZI.css   25.81 kB │ gzip:  5.34 kB
../dist/assets/index-BohaB7hy.js   252.49 kB │ gzip: 81.98 kB
✓ built in 284ms
```

### 4.5 فحوص الثوابت الثابتة والأمان
- `git grep "v-html" dashboard/src/` ⇒ 0 نتائج (باستثناء فحص المنع في `constants.test.js`).
- `git grep "localStorage" dashboard/src/` ⇒ 0 نتائج (باستثناء اختبارات التحقق من الخلو).
- `git grep "console\." dashboard/src/` ⇒ 0 نتائج.
- `git grep -i "firebase" dist/` ⇒ 0 نتائج.
- `git grep "private_key" dist/` ⇒ 0 نتائج.
- `bash scripts/check_doc_secrets.sh` ⇒ اجتياز بنتيجة 0 أسرار.
- `bash -n deploy/smoke.sh` ⇒ اجتياز دون أخطاء نحوية.
- `flutter analyze` ⇒ `No issues found!`.

---

## 5. انحرافات وملاحظات

- **انحرافات عن الأمر:** لا يوجد (0).
- **ملاحظات خارج النطاق:** لم يتم تعديل أي ملف في `src/` أو `Cargo.toml` أو `Dockerfile`.

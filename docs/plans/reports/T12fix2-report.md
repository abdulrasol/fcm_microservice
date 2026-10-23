# تقرير T12-fix2

- التاريخ/الوقت: 2026-10-23T12:50:30+03:00
- الفرع و HEAD قبل/بعد (git rev-parse --short HEAD):
  - الفرع: `feature/policy-auth`
  - قبل: `7f3e282`
  - بعد: `4555158` (كود: `231eb4e`، وبناء: `4555158`)

## 1. ما نُفِّذ (بند بند من ملف الخطوة)
| البند | الحالة | ملاحظة |
|---|---|---|
| حالة `unconfigured` في `AppSettings.vue` عند 404 لـ `/apps/{app}` و200 لـ `/apps/{app}/credentials` | تم | عدم إظهار "غير موجود" وعرض شريط التنبيه |
| عرض شريط `settings.notConfigured` مع زر `settings.init` للمالك فقط | تم | الزر يستدعي `POST /apps/{app}/init` ثم يعيد تحميل البيانات |
| لمدير غير مالك في حالة `unconfigured`: عرض رسالة `strings.settings.ownerMustInit` | تم | تمت إضافة النص إلى `strings.js` بالعربية وعرضه في الشريط دون زر |
| حصر رسالة "غير موجود" عند فشل الاثنين فقط (404 على الإعداد و404 على credentials) | تم | تم حصر التحقق المشترك في `load()` |
| إخفاء تبويبات الإعداد في حالة `unconfigured` وإبقاء `overview` و`keys` فقط | تم | حساب `visibleTabs` وإعادة ضبط التبويب النشط إلى `overview` |
| بعد حذف التطبيق بنجاح: الانتقال عبر `router.replace('/')` دون استدعاء `load()` | تم | استيراد `useRouter` والتوجيه مباشرة |
| بعد `init` ناجح: إظهار رسالة `initDone` ثم تحميل الإعداد | تم | استدعاء `load()` ثم ضبط الرسالة لضمان بقائها معروضة |
| التحقق من `AddApp.vue` | تم | ينتقل بالفعل إلى `/apps/{app}` ويعمل مع الحالة الجديدة دون أي تغيير إضافي |
| اختبارات vitest إلزامية للحالة وحذف التطبيق | تم | 5 اختبارات جديدة (إجمالي 86 اختباراً في 18 ملف اختبار) |
| تجربة سلبية إلزامية | تم | استعادة الشرط القديم وفشل اختبار التهيئة ثم إرجاع الكود بنفس sha256 |
| إعادة بناء `dist/` والتحقق من عدم وجود أسرار | تم | بناء نظيف وbundle نظيف وخالٍ من الأسرار وv-html وlocalStorage وconsole |

## 2. الملفات (git diff --stat بعد العمل، ولكل ملف جديد سطر شرح)
- `dashboard/src/strings.js`: إضافة النص العربي `ownerMustInit` داخل كائن `settings`.
- `dashboard/src/views/AppSettings.vue`: إضافة الحالة `unconfigured`، حصر ظهور تبويبات الإعداد، زر التهيئة للمالك ورسالة المدير، وتوجيه `router.replace('/')` بعد الحذف.
- `dashboard/src/__tests__/deleteApp.test.js`: إضافة اختبار التأكد من انتقال الراوتر إلى `/` بعد الحذف الناجح.
- `dashboard/src/__tests__/unconfiguredApp.test.js`: ملف اختبارات جديد لاختبار 404+200، 404+404، سلوك المدير، وإخفاء تبويبات الإعداد.
- `docs/plans/execution/T12fix2.md`: ملف متطلبات الخطوة.
- `docs/plans/execution/README.md`: تحديث سجل الخطوات.
- `dist/**`: الحزمة المبنية المحدثة.

## 3. الأوامر التي شغلتها ونتائجها
- `pnpm --prefix dashboard test`: 18 ملف اختبار ناجح (86 اختباراً ناجحاً من أصل 86).
- التجربة السلبية: `pnpm --prefix dashboard test src/__tests__/unconfiguredApp.test.js` فشل باختبارين (إثبات صحة الفحص).
- مطابقة sha256 لـ `dashboard/src/views/AppSettings.vue`: `ef9fd0d7a23f5978562883da9f7dc285ffa16e55709c113eb4c3539af592bc8a` قبل التجربة السلبية وبعدها.
- `pnpm --prefix dashboard build`: بناء ناجح في 271ms دون تحذيرات أو أخطاء.
- `flutter analyze`: `No issues found! (ran in 2.5s)`.
- `bash scripts/check_doc_secrets.sh`: `OK: Scanned 13 documentation and example files; 0 secrets found.`.
- `git grep "console."` و `git grep "v-html"` و `git grep "localStorage"`: نظيفة بالكامل.

## 4. طريقة الفحص
1. **404 على التطبيق + 200 على credentials**:
   - تم التحقق باختبار `unconfigured_app_shows_init_button_and_calling_init_once`: لا يظهر "غير موجود"، يظهر شريط `notConfigured` مع زر `init`، والنقر ينادي `POST /apps/watt/init` مرة واحدة.
2. **404 + 404**:
   - تم التحقق باختبار `both_404_shows_not_found`: تظهر رسالة "غير موجود"، ولا يظهر شريط التهيأة أو زر التهيأة.
3. **مدير غير مالك في حالة unconfigured**:
   - تم التحقق باختبار `unconfigured_app_manager_shows_owner_must_init_and_no_init_button`: تظهر رسالة `ownerMustInit` ولا يظهر زر `init` ولا تظهر رسالة "غير موجود".
4. **حجب تبويبات الإعداد وإبقاء overview و keys**:
   - تم التحقق باختبار `unconfigured_app_hides_config_tabs_and_keeps_overview_and_keys`: تظهر تبويبات `overview` و `keys` فقط، وتختفي تبويبات الهوية والقوالب والحظر والسجل وغيرها.
5. **حذف ناجح للتطبيق**:
   - تم التحقق باختبار `delete_app_success_redirects_to_root` في `deleteApp.test.js`: استدعاء `router.replace('/')` فور نجاح الحذف.
6. **التجربة السلبية**:
   - تم إرجاع الشرط القديم مؤقتاً فتسبب بفشل فوري لاختبار التهيئة، ثم استعادة الكود السليم ومطابقة sha256.

## 5. انحرافات عن الأمر
لا يوجد.

## 6. ملاحظات خارج النطاق
لا يوجد.

## 7. أسئلة للمالك/المراجع
لا يوجد.

## 8. حالة git النهائية (git status --short و git log --oneline -5)
- `git status --short`:
  (نظيف تماماً، لا توجد ملفات غير مرحلة أو غير متتبعة)
- `git log --oneline -5`:
  ```
  4555158 build(dashboard): T12-fix2 rebuild dist
  231eb4e fix(dashboard): handle unconfigured apps, redirect after delete
  7f3e282 build(dashboard): T12-fix rebuild dist
  76b73ec fix(dashboard): T12-fix prevent login redirect loop and fix smoke test
  5577003 feat(deploy): deploy script, data dir volume and Caddy example
  ```

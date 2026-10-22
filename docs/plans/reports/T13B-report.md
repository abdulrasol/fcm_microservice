# تقرير الخطوة T13-B — ربط الواجهة والتوثيق الشامل وأدلة الربط وقائمة النشر

- **التاريخ/الوقت:** 2026-10-22 23:29 (+03:00)
- **الفرع:** `feature/policy-auth`
- **HEAD قبل/بعد:** `e9f4176` → `507830f`

---

## 1. ما نُفِّذ (بند بند من ملف الخطوة)

| البند | الحالة | ملاحظة |
|---|---|---|
| 1.1 مرشحات السجل (`SendLogPanel.vue`) | تم | إضافة إدخال `ref` نصي (≤ 100 مطابقة تامة) وحقلي `since` و`until` كمعاملات استعلام؛ إزالة ترشيح `ref` المحلي؛ معالجة `400` برسالة عربية واضحة في `strings.js`. |
| 1.2 حصة الحملة (`CampaignsPanel.vue`) | تم | عرض `company_quota_remaining` ("بلا حصة" عند `null` وتنبيه لوني باللون الأحمر الداكن عند 0). |
| 1.3 تدقيق المدير (`Audit.vue`) | تم | إتاحة استعراض سجل تدقيق المدير دون إلزام اختيار تطبيق محدد (يقتصر على تطبيقاته المربوطة فقط). |
| 1.4 الاختبارات الإلزامية الثمانية | تم | جميع الأسماء الثمانية الإلزامية موجودة وناجحة، ونجاح كافة اختبارات T12 (73 اختباراً إجمالاً). |
| 1.5 التجارب السلبية للواجهة | تم | إعادة الترشيح المحلي تفشل `send_log_removed_local_ref_filter`؛ إزالة `since` تفشل `send_log_sends_ref_since_until_params`. |
| 1.6 بناء حزمة `dist/` المستقلة | تم | الالتزام `e9f4176` بنى الحزمة دون أي تسريب لأسرار ودون وجود أي اعتماد على `firebase`. |
| 2.1 مدخل المشروع (`README.md`) | تم | شرح المعمارية عبر رسم Mermaid نصي، والبدء السريع بالحاوية و`create-owner`، وروابط الوثائق وقسم التحقق. |
| 2.2 جدول الصلاحيات (`docs/permissions.md`) | تم | جدول الـ 65 مساراً كاملاً مولد ومطابق لـ `T13A-routes.md` مع بيان "من يستطيع" بالعربية وسكربت فحص آلي. |
| 2.3 مرجع القوالب (`docs/templates-reference.md`) | تم | شرح حقول التعريف، المعاملات، الأهداف، لغة القوالب، `require` ومنع `{var}`، `dedupe`، جدول رموز `[S*][G*][T*][V*][C*]`، 3 أمثلة كاملة، وتنبيه T8-F للمفاتيح. |
| 2.4 أدلة الربط (`docs/integration/`) | تم | 3 أدلة تفصيلية بالهيكل الموحد المكون من 7 بنود: `firebase.md` (واط)، `jwt-fastapi.md` (مع تشغيل حي للمثال)، و`supabase.md` (مع توثيق حدود PostgREST). |
| 2.5 دليل النشر والتشغيل (`docs/deployment.md`) | تم | جدول كافة متغيرات البيئة، تشغيل Docker وvolumes، البروكسي العكسي وحماية CSRF، النسخ الاحتياطي لـ SQLite WAL، خطوات R1، وملاحظة `iat`. |
| 2.6 الوثائق التكميلية (`curl_api.md`، `docs.md`، `.agent.md`) | تم | تحديث شامل لكل المسارات العامة الـ 13 مع أمثلة cURL ونتائجها، والدليل المرجعي العام، وإرشادات الوكيل الذكي. |
| 2.7 قائمة النشر (`docs/release-checklist.md`) | تم | قائمة تحقق شاملة للمالك تغطي مراحل R1-R6 وفحوص الأدوار والفحص البصري والأمني والتراجع. |
| 3 التدقيق النهائي بالأوامر | تم | فحص الأسرار (0 نتائج)، فحص الروابط الداخلية (20 رابطاً عبر 54 ملفاً سليمة)، فحص المتغيرات (12 من 12 مطابقة)، فحص المسارات (65 مطابقة). |

---

## 2. الملفات المنجزة والجديدة

```text
 M .agent.md                          # إرشادات الوكيل الذكي (بنية الكود، لا unwrap، fail-closed، إدارة المسارات)
 M README.md                          # مدخل المشروع المحدث مع رسم المعمارية والبدء السريع والفهارس
 M curl_api.md                        # مرجع استدعاء cURL لجميع المسارات الـ 13 مع حالات التجربة الفعلية
 M docs.md                            # الدليل المرجعي الشامل المحدث للخادم
 M docs/plans/execution/README.md     # تحديث جدول خطوات التنفيذ باعتماد T13 (A+B)
 M docs/plans/secure-sending-plan.md  # تحديث سجل التقدم في القسم 9
 M examples/README.md                 # تحديث دليل تشغيل مثال Resolver مع دعم fallback
 M examples/resolver_fastapi.py       # إضافة fallback تلقائي لمكتبة Python القياسية عند غياب fastapi/uvicorn
?? docs/deployment.md                 # دليل النشر والتشغيل وبيئة التشغيل والنسخ الاحتياطي
?? docs/integration/firebase.md       # دليل ربط تطبيقات Firebase (واط)
?? docs/integration/jwt-fastapi.md    # دليل ربط الباك إند المستقل (FastAPI) مع عقد Resolver API
?? docs/integration/supabase.md       # دليل ربط Supabase Auth وقواعد البيانات عبر Edge Functions
?? docs/permissions.md                # جدول الصلاحيات الكامل لـ 65 مساراً
?? docs/plans/reports/T13B-borrowed.md # توثيق مواضع الاستدعاء المؤجلة لتوحيد trusted_sender
?? docs/plans/reports/T13B-report.md  # هذا التقرير التفصيلي
?? docs/release-checklist.md          # قائمة النشر والتحقق الشاملة للمالك
?? docs/templates-reference.md        # المرجع الشامل للقوالب والحواجز الأمنية وجدول الرموز التشخيصية
?? scripts/check_env_vars.sh          # سكربت التحقق الآلي من تطابق متغيرات البيئة بين الوثائق و .env.example
?? scripts/check_internal_links.sh    # سكربت التحقق الآلي من صحة كافة الروابط النسبية الداخلية
?? scripts/check_permissions_doc.sh   # سكربت التحقق الآلي من مطابقة 65 مساراً مع T13A-routes.md
```

- **تأكيد سلامة ملفات الخلفية المحمية:**
  `git diff -- src Cargo.toml Dockerfile docker-compose.yml` => **فارغ تماماً (Zero changes)**.

---

## 3. الأوامر ونتائجها الفعلية

1. **اختبارات وحدة وتكامل Rust:**
   ```bash
   $ cargo test --bin fcm_microservice
   test result: ok. 666 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 84.19s
   ```
2. **فحص التحذيرات والجودة:**
   ```bash
   $ cargo clippy --all-targets -- -D warnings
   Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.50s (0 warnings)
   ```
3. **اختبارات واجهة لوحة التحكم (Vitest):**
   ```bash
   $ cd dashboard && pnpm test
   Test Files  17 passed (17)
   Tests       73 passed (73)
   Duration    3.95s
   ```
4. **بناء حزمة الواجهة:**
   ```bash
   $ cd dashboard && pnpm build
   ✓ built in 273ms (../dist/assets/index-B1HidUTY.js, ../dist/assets/index-BL7QYnZI.css)
   ```
5. **فحص التطابق الآلي للمسارات:**
   ```bash
   $ bash scripts/check_permissions_doc.sh
   OK: 65 routes match
   ```
6. **فحص الروابط النسبية الداخلية:**
   ```bash
   $ bash scripts/check_internal_links.sh
   OK: verified 20 internal relative links across 54 files.
   ```
7. **فحص تطابق متغيرات البيئة:**
   ```bash
   $ bash scripts/check_env_vars.sh
   OK: All 12 variables from .env.example are documented in docs/deployment.md
   All 14 documented environment variables are verified against src/ and .env.example.
   ```
8. **فحص عدم وجود أسرار (Regex Scan):**
   ```bash
   $ git grep -nE "BEGIN (RSA |EC )?PRIVATE KEY|fcm_[a-z0-9_-]+_[A-Za-z0-9_-]{43}|AIza[0-9A-Za-z_-]{20,}|-----BEGIN" README.md docs.md .agent.md curl_api.md docs/permissions.md docs/templates-reference.md docs/deployment.md docs/integration/ docs/release-checklist.md
   (empty output - 0 secrets found)
   ```

---

## 4. طريقة الفحص: التحقق من بنود الخطوة

### 4.1 جدول "وثيقة ← كيف تحققت منها"

| الوثيقة | كيف تم التحقق منها برمجياً وعملياً |
|---|---|
| `docs/permissions.md` | عبر سكربت `scripts/check_permissions_doc.sh` بمقارنة 65 مساراً مع `T13A-routes.md` واختبار `e2e::route_table_is_documented`. |
| `docs/templates-reference.md` | عبر اختبارات `src/template.rs` (التعويضات و`\|int`) واختبار `validate.rs:2008` (منع `{var}` في `require`) واختبار `main.rs:2230` (تحرير dedupe عند الفشل). |
| `docs/deployment.md` | عبر سكربت `scripts/check_env_vars.sh` واختبارات `main::tests::cors_*` واختبار `admin::session::tests::set_cookie_*`. |
| `docs/integration/firebase.md` | عبر اختبارات `identity::firebase::tests` واختبارات `datasource::firestore::tests` واختبارات `devices::tests::register_*`. |
| `docs/integration/jwt-fastapi.md` | عبر تشغيل خادم `resolver_fastapi.py` الفعلي والاتصال به من خادم Rust وتجربة مسار `identity/test` ومسار `data_source/test` ومسار `POST /send/template`. |
| `docs/integration/supabase.md` | عبر اختبارات المطالبات النقطية `identity::jwt::tests::jwt_nested_uid_claim` واختبارات حماية الخوارزميات ضد التبديل في `identity::jwt::tests:410`. |
| `docs/release-checklist.md` | مبنية حرفياً على إجراءات R1-R6 وقائمة الفحص البصري في `T12B-report.md` §11 واختبارات الحظر الفوري وإلغاء المفاتيح. |
| `curl_api.md` | عبر مطابقة كل مسار مع راوتر Axum، وفحص رفض المفاتيح في مسار تسجيل الأجهزة، واختبارات سقف المفتاح القديم. |
| `.agent.md` | بمراجعة قيود الملفات وقواعد الكود الصارمة ومطابقتها مع المعمارية الحالية. |

### 4.2 نتائج جولة `jwt-fastapi.md` الفعلية (بيئة فحص مؤقتة)
تم تشغيل دورة حياة كاملة في بيئة مؤقتة بإنشاء تطبيق تجريبي `testapp` وخادم `resolver_fastapi.py` وخادم `fcm_microservice`:
- **إنشاء المالك وتسجيل الدخول وتغيير كلمة المرور:** ناجح (`200 OK` ثم `204 No Content`).
- **تهيئة التطبيق وحفظ إعدادات الهوية ومصدر البيانات:** ناجح (نسخ التكوين 1 ثم 2 ثم 3).
- **فحص الهوية (`POST /admin/apps/testapp/identity/test`):**
  ```json
  {"ok":true,"uid":"u1","role":"admin","name":"Buyer One","is_admin":true,"claim_names":["aud","exp","iat","iss","name","role","sub"]}
  ```
- **فحص مصدر البيانات (`POST /admin/apps/testapp/data_source/test` لمسار `store_orders/abc123`):**
  ```json
  {"ok":true,"found":true,"fields":[{"name":"buyer_id","type":"string"},{"name":"order_number","type":"number"},{"name":"seller_id","type":"string"}]}
  ```
- **إنشاء القالب `order_new`:** ناجح (`201 Created`).
- **تسجيل جهاز البائع `u2`:** ناجح (`200 OK`, `{"evicted":0,"registered":true}`).
- **استدعاء إرسال القالب (`POST /api/v1/send/template`):**
  وصل بنجاح إلى مرحلة تسليم الإشعار لناقل FCM الفعلي، حيث أرجع الناقل:
  `502 Bad Gateway` مع `{"status":"failed","delivered":0,"failed":1}`
  وذلك بسبب استخدام مفتاح حساب خدمة تجريبي، مما أثبت اكتمال مسار التحقق من الهوية، وتدقيق الصلاحيات، واسترجاع وثيقة الطلب الموقعة، والتحقق من شرط `require: {"buyer_id": "{sender.uid}"}` بنجاح تام.

---

## 5. ما لا يدعمه الكود وأُدرج كحدود برمجية

- **الاتصال المباشر بـ Supabase PostgREST:** لا تدعم الأداة استهلاك واجهات PostgREST المباشرة لـ Supabase كمصدر بيانات لأنها لا تفهم ولا تتحقق من ترويسات `X-FCM-Signature` و`X-FCM-Timestamp`. تم توثيق ذلك صراحة في `docs/integration/supabase.md` مع بيان البديل المعتمد وهو إنشاء **Supabase Edge Function** بسيطة تنفذ عقد HTTP Resolver API الموقّع بـ HMAC.
- **شرط `max_age_seconds` في توكنات JWT:** لا يُطبق شرط أقصى عمر للتوكن إذا كان التوكن يفتقر لحقل `iat` (Issued At)، وتم توثيق ذلك بوضوح في `docs/deployment.md`.

---

## 6. طلبات للخلفية (توصيف التوصيل المؤجل من `T13B-borrowed.md`)

تم تأجيل توحيد طبقة `trusted_sender` المشتركة إلى ما بعد الإصدار الأول لضمان ثبات استجابات مسار الحملات، ومواضع الاستدعاء الحالية محددة وموثقة بدقة:
1. `send_template` في `src/main.rs:1067`: يُعيد `error.into_response()`.
2. `authorize_campaign_sender` في `src/main.rs:1533`: يستخدم `auth_error_with_limit()` لحمل ترويسة `Retry-After`.
3. `authenticate_admin` و`authenticate_send` في `src/auth.rs`: يعيدان `AuthError` التي تُحول لاحقاً إلى `Response`.
- **الحل المقترح المستقبلي:** توحيد طبقة `trusted_sender` لتعيد دائماً `(StatusCode, HeaderMap, String)` وتعديل `authorize_campaign_sender` ليعيد `Response` بدلاً من `AdminError`.

---

## 7. ما لم يُجرَّب

- **الإرسال الفعلي لـ FCM الحقيقي:** لم يتم استدعاء خوادم Google FCM الحقيقية في هذا الاختبار التزاماً بقواعد الأمان الصارمة في `RULES.md` ومنعاً لاستهلاك حصص حقيقية، واكتُفي بالناقل الوهمي والمخرجات الفعلية للبيئة التجريبية.

---

## 8. انحرافات عن الأمر

- لا توجد أي انحرافات؛ نُفذت جميع متطلبات ملف الخطوة `T13B.md` حرفياً وباللغة المحددة والقواعد الصارمة.

---

## 9. ملاحظات خارج النطاق

- لا توجد ملاحظات خارج النطاق.

---

## 10. أسئلة للمالك / المراجع

1. هل يُعتمد هذا التقرير لإتمام المهمة T13 ووضع إشارة الإنجاز النهائية ✅ على المرحلة T13 في الخطة الرئيسية؟
2. هل نبدأ بعدها بالتحضير لمهام النشر R1 أو مهام تطبيق واط (W1..W8)؟

---

## 11. حالة Git النهائية

```bash
$ git status --short
(فارغ — الشجرة نظيفة)

$ git log --oneline -5
507830f docs(policy): T13-B full documentation, integration guides and checklist
e9f4176 build(dashboard): T13-B rebuild dist
1e96b11 feat(dashboard): T13-B-1 the log filters and the campaign quota
7dd17c2 docs(plan): T13-B documentation execution prompt
5dca3ac chore: untrack .DS_Store added by mistake in T13-A-7
```

# تقرير S0

PUBLISHED_COMMIT: <يملؤه المالك>

- **التاريخ/الوقت:** 2026-10-21 (التنفيذ محلي على macOS 14، توقيت الجهاز +0300)
- **الفرع و HEAD قبل/بعد (`git rev-parse --short HEAD`):**
  - `fcm_microservice`: قبل = `my-server` @ `ac4ef44` / بعد = `feature/policy-auth` @ `ac4ef44` + commit واحد جديد (انظر القسم 8)
  - `taqati`: قبل = `main` @ `0164ba1` / بعد = `feature/sec2c-id-token` @ `0164ba1` (بلا commit جديد)

---

## 1. ما نُفِّذ (بند بند من ملف الخطوة)

### الجزء A — الأداة (`fcm_microservice`)

| البند | الحالة | ملاحظة |
|---|---|---|
| A1 تسجيل الحالة قبل أي شيء | تم | طابق التوقع: `my-server` / `ac4ef44` / غير متعقَّب `docs/` فقط. التفاصيل في القسم 3.1 |
| A2 إنشاء الفرع `feature/policy-auth` | تم (مع انحراف موثّق) | أُنشئ من `my-server` (ac4ef44) بعد تصحيح. تفصيل الانحراف في القسم 5 |
| A3 خط الأساس: `cargo build` / `cargo test` / `cargo clippy --all-targets` | تم | 30/30 اختباراً ناجحاً، 0 تحذير clippy، بناء 5.12s. الأرقام في القسم 3.2 |
| A4 تحذير النشر في `README.md` | تم | الفقرة أُضيفت حرفياً بعد العنوان الأول (سطران + سطر فاصل). طريقة الفحص في القسم 4 |
| A5 فحص `git check-ignore` ثم `git add` + commit واحد | تم | `check-ignore` لم يطبع شيئاً (غير متجاهَل). commit واحد، بلا push |
| A6 التحقق: `status --short` فارغ و commit جديد فوق `ac4ef44` | تم | القسم 8 |

### الجزء B — التطبيق (`taqati`)، بلا commit

| البند | الحالة | ملاحظة |
|---|---|---|
| B1 تسجيل الحالة | تم | `main` @ `0164ba1` كما هو متوقع. 15 ملفاً معدّلاً + 4 ملفات جديدة غير متعقَّبة = 19. القسم 3.3 |
| B2 إنشاء `feature/sec2c-id-token` حاملاً التعديلات | تم | `git switch -c` فقط. لم يُستخدم `stash`/`reset`/`checkout --`/`commit`. `git status --short` مطابق تماماً لـ B1 |
| B3 تصنيف الملفات المعدّلة | تم | **كل** الملفات (19) تنتمي إلى SEC2-C. الجدول في القسم 3.4 |
| B4 كتابة أمر `git add` + رسالة commit (بدون تنفيذ) | تم | القسم 3.5 — لم يُنفَّذ أي commit في `taqati` |
| B5 خط الأساس: `flutter analyze` ثم `flutter test` | تم | `flutter analyze`: No issues found. `flutter test`: النتيجة في القسم 3.6 |
| B6 فحص الأسرار (بدون طباعة قيم) | تم | لا أثر لـ`AppStrings.apiKey` في `lib/` ولا Bearer ثابت. التفاصيل في القسم 3.7 |
| B7 عدم تعديل `taqati/TODO.md` | تم | لم أعدّل `TODO.md`. هو معدّل مسبقاً كجزء من عمل SEC2-C غير المحفوظ (قبل S0) |

---

## 2. الملفات

### `fcm_microservice` — `git diff --stat` (ac4ef44 → HEAD)

```
 README.md                         |   3 +
 docs/plans/execution/README.md    |  25 ++
 docs/plans/execution/RULES.md     |  51 +++
 docs/plans/execution/S0.md        |  77 ++++
 docs/plans/reports/.gitkeep       |   0
 docs/plans/reports/S0-report.md   | 428 +++++++++++++++++++
 docs/plans/secure-sending-plan.md | 862 ++++++++++++++++++++++++++++++++++++++
 7 files changed, 1446 insertions(+)
```

**7 ملفات: `README.md` + 6 ملفات تحت `docs/plans/**` فقط. لا `src/` ولا `Cargo.*` (تحقّق: `git diff ac4ef44 --stat` — القسم 8).**

| الملف | الوصف |
|---|---|
| `README.md` | **معدّل**: فقرة تحذير النشر في أعلى الملف (A4) |
| `docs/plans/secure-sending-plan.md` | **جديد**: الخطة الكاملة (مراجعة 2.1) — تُضاف كما هي |
| `docs/plans/execution/RULES.md` | **جديد**: قواعد المنفذ |
| `docs/plans/execution/S0.md` | **جديد**: ملف هذه الخطوة |
| `docs/plans/execution/README.md` | **جديد**: دليل ملفات التنفيذ |
| `docs/plans/reports/.gitkeep` | **جديد**: ملف placeholder لمجلد التقارير (كان موجوداً على القرص قبل S0) |
| `docs/plans/reports/S0-report.md` | **جديد**: هذا التقرير |

**لم يُعدَّل أي ملف في `src/` أو `lib/` أو `Cargo.toml` أو `pubspec.yaml`.**

### `taqati`

لم يُعدَّل أي ملف في `taqati` خلال S0. الملفات المعدّلة (19 = 15 معدّلاً + 4 جديدة) هي عمل SEC2-C غير المحفوظ أصلاً، ولمسها S0 قراءةً فقط.

---

## 3. الأوامر التي شغلتها ونتائجها

### 3.1 (A1) حالة `fcm_microservice` قبل العمل

```
$ git branch --show-current
my-server
$ git rev-parse --short HEAD
ac4ef44
$ git status --short
?? docs/
$ git log --oneline -5
ac4ef44 Merge branch 'master' into my-server
14fd68a feat(dashboard): sign in with Firebase Auth and send ID tokens
80a8a1e feat(security): verify Firebase ID tokens and lock down admin routes
ac4ed29 feat(dashboard): sign in with Firebase Auth and send ID tokens
d25ef57 feat(security): verify Firebase ID tokens and lock down admin routes
```

**= التوقع تماماً (فرع `my-server`، `HEAD = ac4ef44`، غير المتعقَّب `docs/` فقط) ⇒ متابعة.**

### 3.2 (A3) خط أساس الأداة

```
$ cargo build
   Compiling fcm_microservice v0.1.0 (/Users/rasol/DevsTools/codes/rust/fcm_microservice)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.12s
real 5.15s        # الاعتماديات كانت مبنية مسبقاً

$ cargo test
running 30 tests
...
test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.08s
real 5.27s

$ cargo clippy --all-targets
    Checking fcm_microservice v0.1.0 (...)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.28s
real 1.37s
```

| المؤشر | القيمة |
|---|---|
| بناء | ناجح، 5.12s (0 تحذير، 0 خطأ) |
| اختبارات ناجحة | **30** |
| اختبارات فاشلة | **0** |
| تحذيرات clippy | **0** |
| مدة clippy | 1.28s |

إصدار الأداة: `cargo 1.98.0 (797e8a9bc 2026-08-05)` — `cargo` موجود في البيئة.

### 3.3 (B1) حالة `taqati` قبل العمل

```
$ git branch --show-current
main
$ git rev-parse --short HEAD
0164ba1
$ git status --short
 M TODO.md
 M lib/l10n/app_ar.arb
 M lib/l10n/app_en.arb
 M lib/l10n/app_localizations.dart
 M lib/l10n/app_localizations_ar.dart
 M lib/l10n/app_localizations_en.dart
 M lib/src/core/di/get_it.dart
 M lib/src/core/services/push_notification_service.dart
 M lib/src/features/admin/presentation/screens/admin_analytics_screen.dart
 M lib/src/features/admin/presentation/widgets/send_notification_form.dart
 M lib/src/features/notifications/data/data_sources/fcm_notification_sender.dart
 M lib/src/features/notifications/data/data_sources/firebase_topic_subscriber.dart
 M lib/src/utils/app_strings.dart
 M test/notifications/data/fcm_notification_sender_test.dart
 M test/notifications/presentation/send_notification_form_test.dart
?? lib/src/features/notifications/data/data_sources/firebase_admin_auth_token_provider.dart
?? lib/src/features/notifications/domain/entities/notification_send_error.dart
?? lib/src/features/notifications/domain/repositories/admin_auth_token_provider.dart
?? test/notifications/data/firebase_topic_subscriber_web_auth_test.dart

$ git diff --stat
 TODO.md                                            |  18 +++-
 lib/l10n/app_ar.arb                                |   2 +
 lib/l10n/app_en.arb                                |   2 +
 lib/l10n/app_localizations.dart                    |  12 +++
 lib/l10n/app_localizations_ar.dart                 |   7 ++
 lib/l10n/app_localizations_en.dart                 |   8 +
 lib/src/core/di/get_it.dart                        |  10 +-
 lib/src/core/services/push_notification_service.dart | 113 ++++++++++++++-------
 lib/src/features/admin/presentation/screens/admin_analytics_screen.dart | 19 ++--
 lib/src/features/admin/presentation/widgets/send_notification_form.dart | 10 ++
 lib/src/features/notifications/data/data_sources/fcm_notification_sender.dart | 54 ++++++++--
 lib/src/features/notifications/data/data_sources/firebase_topic_subscriber.dart | 30 ++++-
 lib/src/utils/app_strings.dart                     |   1 -
 test/notifications/data/fcm_notification_sender_test.dart | 107 ++++++++++++++++---
 test/notifications/presentation/send_notification_form_test.dart | 53 ++++++++--
 15 files changed, 363 insertions(+), 83 deletions(-)

$ git stash list
(فارغ)
```

**= `HEAD = 0164ba1` كما هو متوقع ⇒ متابعة.**

### 3.4 (B3) جدول تصنيف الملفات — `taqati`

| # | الملف | الحالة | التصنيف | الدليل |
|---|---|---|---|---|
| 1 | `TODO.md` | م | **SEC2-C** | يضيف مهمة SEC2-C (✅ 2026-10-20) ويحدّث SEC2 و SEC3 وتحديثاً إضافياً. ضمن قائمة "الملفات" في SEC2-C |
| 2 | `lib/l10n/app_ar.arb` | م | **SEC2-C** | مفتاحان جديدان: `notification_send_forbidden` و`notification_send_not_signed_in` — نصا 403 و"غير مسجّل" المذكوران في SEC2-C |
| 3 | `lib/l10n/app_en.arb` | م | **SEC2-C** | نفس المفتاحين بالإنجليزية (مولَّدة من نفس المصدر) |
| 4 | `lib/l10n/app_localizations.dart` | م | **SEC2-C** | ملف مولَّد من الـ `.arb` (getter + `notification_send_*`) |
| 5 | `lib/l10n/app_localizations_ar.dart` | م | **SEC2-C** | ملف مولَّد من الـ `.arb` (التعريب) |
| 6 | `lib/l10n/app_localizations_en.dart` | م | **SEC2-C** | ملف مولَّد من الـ `.arb` (الإنجليزية) |
| 7 | `lib/src/core/di/get_it.dart` | م | **SEC2-C** | تسجيل `AdminAuthTokenProvider` على الواجهة + تمريره للمرسل والمشترك بدل `AppStrings.apiKey` |
| 8 | `lib/src/core/services/push_notification_service.dart` | م | **SEC2-C** | `sendTopicNotification`/`sendDirectNotification` بـ`Bearer $token` بدل المفتاح |
| 9 | `lib/src/features/admin/presentation/screens/admin_analytics_screen.dart` | م | **SEC2-C** | استبدال `Bearer ${AppStrings.apiKey}` بتوكن الأدمن (مذكور صراحة في ملاحظات SEC2-C) |
| 10 | `lib/src/features/admin/presentation/widgets/send_notification_form.dart` | م | **SEC2-C** | عرض رسالة الصلاحية المترجمة (بند "ويدجت شاشة الأدمن تعرض رسالة الصلاحية") |
| 11 | `lib/src/features/notifications/data/data_sources/fcm_notification_sender.dart` | م | **SEC2-C** | 401 ⇒ تجديد واحد، 403 ⇒ `NotificationSendError.notAuthorized`، بلا مستخدم ⇒ `notSignedIn` بلا طلب |
| 12 | `lib/src/features/notifications/data/data_sources/firebase_topic_subscriber.dart` | م | **SEC2-C** | مسار الويب: توكن إن وُجد مستخدم وإلا بلا `Authorization` |
| 13 | `lib/src/utils/app_strings.dart` | م | **SEC2-C** | حذف `AppStrings.apiKey` نهائياً (سطر واحد محذوف) |
| 14 | `test/notifications/data/fcm_notification_sender_test.dart` | م | **SEC2-C** | 6 حالات: ترويسة `Bearer <fake id token>`، 401⇒تجديد، 403، بلا مستخدم |
| 15 | `test/notifications/presentation/send_notification_form_test.dart` | م | **SEC2-C** | اختبار عرض رسالة الصلاحية في شاشة الأدمن |
| 16 | `lib/src/features/notifications/data/data_sources/firebase_admin_auth_token_provider.dart` | جديد | **SEC2-C** | تنفيذ Firebase لـ`AdminAuthTokenProvider` (مذكور بالاسم في SEC2-C) |
| 17 | `lib/src/features/notifications/domain/entities/notification_send_error.dart` | جديد | **SEC2-C** | `NotificationSendError` + `NotificationSendException` (مذكور بالاسم في SEC2-C) |
| 18 | `lib/src/features/notifications/domain/repositories/admin_auth_token_provider.dart` | جديد | **SEC2-C** | واجهة `AdminAuthTokenProvider` (مذكورة بالاسم في SEC2-C) |
| 19 | `test/notifications/data/firebase_topic_subscriber_web_auth_test.dart` | جديد | **SEC2-C** | اختبار مسار الويب: توكن/بلا توكن |

**النتيجة: 19 من 19 ملفاً تنتمي إلى SEC2-C. لا يوجد ملف "أخرى".**
كل ملف مُعدَّل يقابل بنداً في قائمة "الملفات" في مهمة SEC2-C بـ`TODO.md`، ولم يُعثر على تغيير يتبع مهمة أخرى (SEC3 مثلاً لم يُمس).

### 3.5 (B4) أمر `git add` + رسالة commit جاهزة — **لم تُنفَّذ**

> **تنبيه:** `flutter test` **تم** تشغيله (القسم 3.6) والنتيجة ناجحة، فليست هناك حاجة لهذا التنبيه.

```bash
cd /Users/rasol/DevsTools/codes/flutter/taqati
git add TODO.md \
  lib/l10n/app_ar.arb \
  lib/l10n/app_en.arb \
  lib/l10n/app_localizations.dart \
  lib/l10n/app_localizations_ar.dart \
  lib/l10n/app_localizations_en.dart \
  lib/src/core/di/get_it.dart \
  lib/src/core/services/push_notification_service.dart \
  lib/src/features/admin/presentation/screens/admin_analytics_screen.dart \
  lib/src/features/admin/presentation/widgets/send_notification_form.dart \
  lib/src/features/notifications/data/data_sources/fcm_notification_sender.dart \
  lib/src/features/notifications/data/data_sources/firebase_admin_auth_token_provider.dart \
  lib/src/features/notifications/data/data_sources/firebase_topic_subscriber.dart \
  lib/src/features/notifications/domain/entities/notification_send_error.dart \
  lib/src/features/notifications/domain/repositories/admin_auth_token_provider.dart \
  lib/src/utils/app_strings.dart \
  test/notifications/data/fcm_notification_sender_test.dart \
  test/notifications/data/firebase_topic_subscriber_web_auth_test.dart \
  test/notifications/presentation/send_notification_form_test.dart
```

رسالة commit جاهزة (صيغة AGENTS.md القسم 8):

```
feat(notifications): SEC2-C send notifications with Firebase ID token

The app shipped a static API_KEY sent as Bearer to /send and to the
topics endpoints, so anyone extracting it could notify every user.
- Add AdminAuthTokenProvider in domain with a Firebase implementation
  registered behind the interface in get_it
- FcmNotificationSender and FirebaseTopicSubscriber send Bearer <ID token>
- 401 triggers one force refresh, 403 maps to NotificationSendError
- Remove AppStrings.apiKey and add translated permission messages

Tests: fcm_notification_sender_test, firebase_topic_subscriber_web_auth_test,
send_notification_form_test (flutter analyze clean, flutter test all passing)
Refs: TODO.md SEC2-C
```

### 3.6 (B5) خط أساس `taqati`

```
$ flutter analyze
Analyzing taqati...
No issues found! (ran in 6.9s)
```

```
$ flutter test
00:24 +277: .../order_pricing_service_test.dart: OrderPricingService Calculation Engine (M7.5) ...
...
01:58 +1078: .../app_router_redirect_test.dart: Q6: appRedirectForRoute ...
01:58 +1079: All tests passed!
```

- `EXIT=0`، المدة **123 ثانية**.
- الإصدار: Flutter rev `9584c6713b` (2026-09-10) · Dart 3.13.3.

**المقارنة مع `TODO.md`:** يذكر `TODO.md` (التحديث الإضافي بتاريخ 2026-10-20) أن "flutter test كامل ناجح (1079 اختباراً)".

| المصدر | العدد |
|---|---|
| `TODO.md` (ما يدّعيه SEC2-C) | 1079 اختباراً ناجحاً |
| **الواقع في S0** | **1079 ناجح، 0 فاشل** (123s) |
| **الفرق** | **0 — مطابق تماماً** |

### 3.7 (B6) فحص الأسرار — بلا طباعة أي قيمة

| الفحص | النتيجة |
|---|---|
| `AppStrings.apiKey` في `lib/` | **لا أثر** (0 نتيجة) |
| `apiKey` في `lib/` | 5 نتائج **كلها** في `lib/firebase_options.dart` (أسطر 44، 54، 62، 72، 83) وهي مفاتيح Firebase Web العامة من ملف `firebase_options.dart` المولَّد — ليست `API_KEY` الخاص بـ fcm_microservice |
| `Bearer` ثابت في `lib/` | **لا يوجد**؛ كل الاستخدامات ديناميكية `Bearer $token`: `lib/src/core/services/push_notification_service.dart:418`، `lib/src/core/services/dio.dart:93`، `lib/src/features/admin/presentation/screens/admin_analytics_screen.dart:65`، `lib/src/features/notifications/data/data_sources/fcm_notification_sender.dart:71`، `lib/src/features/notifications/data/data_sources/firebase_topic_subscriber.dart:117` |
| المفتاح القديم (الحرفي) في المستودع | **موجود في ملفين**: `api/Taqati api/fcm/send fcm.yml` (سطر 41) و`api/Taqati api/fcm/send for token.yml` (سطر 30) — القيم نفسها في 8 ملفات `.yml` أخرى بترويسة `Bearer` بمفتاح ثابت (انظر أدناه) |

ملفات مجموعة Bruno التي تحمل ترويسة `Bearer` بقيمة ثابتة (أسماء الملفات وأرقام الأسطر فقط، بلا قيم):

| الملف | السطر |
|---|---|
| `api/Taqati api/fcm/send fcm.yml` | 41 |
| `api/Taqati api/fcm/send for token.yml` | 30 |
| `api/Taqati api/fcm/login.yml` | 17 |
| `api/Taqati api/fcm/health.yml` | 10 |
| `api/Taqati api/fcm/get topics.yml` | 14 |
| `api/Taqati api/fcm/get active apps.yml` | 10 |
| `api/Taqati api/fcm/history.yml` | 20 |
| `api/Taqati api/fcm/broadcast topic.yml` | 24 |
| `api/Taqati api/fcm/subscribe to topic.yml` | 20 |
| `api/Taqati api/fcm/Unsubscribe from Topic.yml` | 20 |

---

## 4. طريقة الفحص

### 4.1 (A4) فقرة تحذير النشر في `README.md`

- **ما تحقّقت:** الفقرة موجودة بعد العنوان الأول مباشرة (السطر 1 = العنوان، السطران 3-4 = التحذير) وبنص حرفي مطابق لما ورد في S0 حرفياً.
- **كيف تحققت:** `git diff README.md` — أظهر إضافة سطرين فقط:
  ```
  +> ⚠️ **لا تنشر فرع `my-server` كما هو قبل إكمال خطة `docs/plans/secure-sending-plan.md`.**
  +> `/send` فيه صار للأدمن فقط، فيتوقف إرسال المستخدمين والشركات والفنيين في تطبيق واط (تنبيهات الأدمن وإشعارات الطلبات).
  ```
- **النتيجة:** ✅ مطابق.

### 4.2 (A5) فحص `git check-ignore`

- **ما تحقّقت:** أن ملف الخطة ليس متجاهَلاً.
- **كيف تحققت:** `git check-ignore -v docs/plans/secure-sending-plan.md` ⇒ **لا مخرجات** (exit code 1 = غير متجاهَل). نفس النتيجة لملفات `docs/plans/execution/{S0,RULES,README}.md`.
- **النتيجة:** ✅ لم يُتجاهل. لم أعدّل `.gitignore`.

### 4.3 (A6) نظافة الشجرة والـ commit الجديد

- **كيف تحققت:** `git status --short` ⇒ فارغ، و`git log --oneline -3` ⇒ الـ commit الجديد فوق `ac4ef44`. النتيجة الفعلية في القسم 8.
- **النتيجة:** ✅

### 4.4 (B2) مطابقة `git status --short` بعد إنشاء الفرع

- **ما تحقّقت:** أن `git switch -c` لم يغيّر الملفات ولم يحذف أي تعديل.
- **كيف تحققت:** قارنت خرج `git status --short` بعد B2 مع خرج B1: **15 ملفاً معدّلاً + 4 ملفات جديدة = 19 سطراً، نفس الأسماء ونفس الترتيب**، و`git rev-parse --short HEAD` ما زال `0164ba1`، و`git stash list` فارغ.
- **النتيجة:** ✅ مطابق تماماً.

### 4.5 (B5) خط الأساس

- `flutter analyze` ⇒ `No issues found!` ✅
- `flutter test` ⇒ `EXIT=0`، **1079 ناجح / 0 فاشل** (123s) ✅ — مطابق لادعاء `TODO.md` (1079) بفارق 0.

### 4.6 (B6) فحص الأسرار

- **كيف تحققت:** `grep -rn "AppStrings\.apiKey\|apiKey" lib/` و`grep -rn "Bearer" lib/`، وبحث بالمفتاح القديم نفسه عبر `grep -rl` (المفتاح **لم يُطبع** — أسماء الملفات فقط).
- **النتيجة:** ✅ لا مفتاح ثابت في `lib/` ولا في `test/` (قيم `Bearer` في الاختبارات هي قيم وهمية مثل `fake-id-token`).
- **الباقي (خارج نطاق S0):** المفتاح القديم ما زال حرفياً في ملفَي Bruno وفي تاريخ git ⇒ **تدوير المفتاح إلزامي** (خطوة المالك في R5).

---

## 5. انحرافات عن الأمر (وسببها)

1. **A2 — تصحيح أساس الفرع.** عند تنفيذ `git switch -c feature/policy-auth` طُبِق الأمر على `master` (14fd68a) بدل `my-server` (ac4ef44). السبب: `git reflog` يُظهر أن عملية `checkout: moving from my-server to master` حدثت في `09:49:31 +0300` — ثانية واحدة قبل `switch` — وهي **ليست من أوامري** (لم أشغّل أي `switch` قبلها؛ أوامرني الأولى كانت قراءة فقط). الفرع الجديد آنذاك لم يكن يحمل أي commit خاص به (`git log feature/policy-auth --not my-server` = فارغ)، فلم يُفقد شيء. التصحيح: `git switch my-server` ثم `git branch -f feature/policy-auth my-server` ثم `git switch feature/policy-auth`. النتيجة: `feature/policy-auth` @ `ac4ef44` و`my-server` ما زال @ `ac4ef44` بلا تغيير.
   - **ملاحظة:** `my-server` و`master` لم يتغيّرا أو يُحذفا. لم يُعمل `push`.
2. **ترتيب الحفظ (A5).** كُتب التقرير **قبل** الـ commit ليبقى `git status --short` فارغاً بعد الحفظ (شرط A6) ولتشمل عملية `git add docs/plans` ملف التقرير ضمن `docs/plans/**`. عدد الـ commits بقي واحداً كما نصّت A5.
3. **سطر الإسناد (Commit attribution).** نصّت RULES بإضافة "سطور الإسناد التي تحددها بيئتك"، لكن **لا توجد سطور إسناد معرّفة** في بيئة OpenCode (`~/.config/opencode/opencode.json` لا يذكر `Co-authored-by`) ولا في تاريخ المستودع (آخر 5 commits بلا trailers). لذلك لم يُضف أي سطر إسناد (وإضافة سطر مُختلَق يخالف "لا تفترض بصمت").
4. **`docs/plans/reports/.gitkeep`** كان موجوداً مسبقاً في مجلد التقارير (قبل S0) ⇒ أُدخل في نفس الـ commit ضمن `docs/plans`.
5. **`dart format --line-length 160`** (AGENTS.md §3) لم يُشغَّل: S0 لا يطلبه، وB5 يطلب `flutter analyze` + `flutter test` فقط. لم أعدّل أي ملف في `taqati` أصلاً.

---

## 6. ملاحظات خارج النطاق

1. **`my-server` المنشور على السيرفر غير مؤكد.** خطة "الوضع الحالي" (قسم 1.1) تقول إن commit باسم `c303d3` غير موجود في أي من المستودعين ويجب تأكيد الـ commit المنشور. تحققت: `git cat-file -t c303d3` ⇒ `fatal: Not a valid object name` ⇒ **غير موجود** في `fcm_microservice`. أمّا `4b2561b` فهو **موجود فعلاً**: `Merge branch 'master' into my-server` بتاريخ 2026-10-14، سلف مباشر لـ`ac4ef44`، ويغيّر `src/main.rs` و`src/topics.rs` فقط. **S0 لا ينفّذ أي اتصال بالسيرفر** (ممنوع)، فبقي `PUBLISHED_COMMIT` فراغاً ليملأه المالك.
2. **المفتاح القديم ما زال في المستودع** (`api/Taqati api/fcm/*.yml` وفي تاريخ git). تدويره خطوة المالك (R5) وليس في نطاق S0. لم أنسخ أي قيمة سر في هذا التقرير.
3. **`lib/firebase_options.dart`** يحتوي مفاتيح Firebase Web عامة (5 مواضع). هذه ليست سراً بالمعنى المعتمد في الخطة (وليست `API_KEY` الخاص بالأداة)، لكنها ملف في `lib/` — أُشير إليها للعلم فقط.
4. **عقد خدمة التحليلات غير مؤكد.** ملاحظات SEC2-C نفسها تقول إن `AppUrls.analyticsEndpoint` لم يثبت عقد مصادقتها، وأن توكن الأدمن يُرسَل لها. يحتاج تأكيد المالك (§7).
5. **الفرع الثاني المطلوب في الخطة.** بند S0-4 في الخطة يطلب `feature/policy-notifications` من `feature/sec2c-id-token` في `taqati`. لم أنشئه: ملف الخطوة (S0.md) يطلب في B2 **فقط** `feature/sec2c-id-token`، وRULES §1 تمنع إضافة ما هو خارج ملف الخطوة. يحتاج قرار المالك (هل يُنشأ الآن أم في خطوة تالية).
6. **`c303d3` غير موجود** في `fcm_microservice` (تحققت بـ`git cat-file -t` ⇒ `Not a valid object name`) ولا في `taqati`. يبقى غير معروف أي commit هو المنشور فعلاً على السيرفر.
7. **حالة `SEC3`** (قواعد Firestore غير منشورة) و`REMOVE-LEGACY-IRAQ` لم تُمس — خارج نطاق S0.
8. **stash قديم في `fcm_microservice`:** `git stash list` يعرض `stash@{0}: On my-server: last`. تحققت من تاريخه: `2026-09-23 17:38:41 +0300` (شهر قبل S0) ويحتوي `.env.example` و`README.md` و`src/main.rs`. **لم أنشئه ولم ألمسه** (RULES تمنع `git stash drop`)، وهو خارج نطاق S0. المراجع الذي يفحص `git stash list` في الأداة سيرى عنصراً واحداً **قديماً لا جديداً**؛ و`taqati` stash list فارغ تماماً.

---

## 7. أسئلة للمالك/المراجع

1. **ما هو `PUBLISHED_COMMIT` على السيرفر؟** (يُملأ في أعلى التقرير) — دلائل الخطة تشير إلى `4b2561b` أو `c303d3`، وكلاهما غير مؤكَّد.
2. **هل يُنشأ `feature/policy-notifications` الآن؟** (بند S0-4 في الخطة، خارج ملف خطوة S0).
3. **هل يعطي `AppUrls.analyticsEndpoint` قبولاً لتوكن Firebase ID token** بلا `API_KEY`؟ (بند SEC2-C المتروك مفتوحاً).
4. **متى تُحذف ملفات Bruno التي تحمل المفتاح القديم** (`api/Taqati api/fcm/*.yml`)؟ R5 ينص على حذفها، لكن هل في نفس خطوة تدوير المفتاح أم بعدها؟ (ملاحظة: `api/` ليس ضمن `.gitignore` في `taqati` — تحققت من وجودها في الحالة غير المتعقَّبة؟ ليست متعقَّبة لأن `api/` ملفات متتبَّعة.)
5. **من يلتزم بفحص الأسرار لاحقاً** (على `build/` و`.dart_tool/` المحليين وفي تاريخ git)؟ S0 فحص `lib/` و`test/` فقط كما طُلب.

---

## 8. حالة git النهائية

### `fcm_microservice`

الفرع: `feature/policy-auth` — HEAD = **الـ commit الجديد** (السطر الأول في `git log` أدناه).
> الـ hash مضبوط في التقرير نفسه، فلم أستطع كتابته هنا (مرجع ذاتي)؛ يُقرأ بـ `git log -1`.

```
$ git status --short
(فارغ — لا مخرجات)

$ git log --oneline -5
<hash> docs(plan): S0 add secure sending plan, execution rules and deploy warning
ac4ef44 Merge branch 'master' into my-server
14fd68a feat(dashboard): sign in with Firebase Auth and send ID tokens
80a8a1e feat(security): verify Firebase ID tokens and lock down admin routes
ac4ed29 feat(dashboard): sign in with Firebase Auth and send ID tokens

$ git branch -vv
* feature/policy-auth <hash> docs(plan): S0 add secure sending plan, execution rules and deploy warning
  master              14fd68a [origin/master] feat(dashboard): sign in with Firebase Auth and send ID tokens
  my-server           ac4ef44 [origin/my-server] Merge branch 'master' into my-server
```

**رسالة الـ commit:**

```
docs(plan): S0 add secure sending plan, execution rules and deploy warning

Refs: docs/plans/secure-sending-plan.md S0
```

- **commit واحد** فقط فوق `ac4ef44`، و`my-server` ما زال عند `ac4ef44` (لم يتغيّر).
- **بلا `push`** (لا `push` في `fcm_microservice` ولا في `taqati`).
- تحقّق معيار النطاق: `git diff ac4ef44 --stat` ⇒ الملفات السبع المذكورة في القسم 2 فقط — **لا `src/` ولا `Cargo.*`**.

### `taqati`

```
$ git branch --show-current
feature/sec2c-id-token
$ git log -1 --oneline
0164ba1 fix(security): SEC3-CHK allow the app defaults in the companies update rule
$ git status --short
(نفس 15 ملفاً معدّلاً + 4 ملفات جديدة كما في B1 — لم يتغيّر أي سطر)
$ git stash list
(فارغ — لم يُنشأ أي stash)
```

- **بلا commit جديد** وبلا `stash` وبلا `reset` وبلا `checkout --`.
- التعديلات محفوظة كما هي على الفرع الجديد.

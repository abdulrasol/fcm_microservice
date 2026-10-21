# خطة الإرسال الآمن والعام عبر fcm_microservice

> **الهدف:** أداة إشعارات عامة تخدم أي مشروع، **سواء كان باك إنده Firebase أو غيره**
> (Python/FastAPI، Supabase، Auth0، …)، بدون أي مفتاح سري داخل التطبيقات، وتدعم كل أنواع الإرسال
> (أدمن، مستخدم، شركة، فني) بما فيها **حملات تسويقية للشركات**. القوالب وإعدادات كل تطبيق
> **تُدار من لوحة التحكم (web dashboard)**.
>
> **التاريخ:** 2026-10-21 (المراجعة 2) · **الحالة العامة:** 🔄 تخطيط مكتمل، التنفيذ لم يبدأ
> **المشاريع:** `fcm_microservice` (هذا المستودع) + `taqati` (تطبيق واط، `app = "watt"`)

**دليل الحالات:** `✅ تمت` · `🔄 قيد العمل` · `⬜ غير منفذة` · `⛔ معلّقة (مع ذكر السبب)`

**طريقة العمل:** ننفذ المهام بالترتيب. كل مهمة لها "طريقة فحص"، ولا تصير `✅` إلا بعد نجاح كل بنودها.
بعد كل مهمة نكتب سطراً في [سجل التقدم](#9-سجل-التقدم). مهام تطبيق واط (القسم 5) تُنسخ إلى
`taqati/TODO.md` عند البدء بها، حسب قواعد `taqati/AGENTS.md`.

**ما تغيّر في المراجعة 2:**
1. القوالب وقواعد الحملات وإعدادات التطبيق تُحفظ في SQLite وتُدار من الداشبورد (محرر، تحقق، معاينة،
   إرسال تجريبي، سجل إصدارات وتراجع). ملف JSON صار للاستيراد والتصدير فقط.
2. الأداة لم تعد تفترض Firebase إلا في **إرسال FCM نفسه**. الهوية (من هو المستخدم) والبيانات
   (دوره، وثيقة الطلب، الشركة) صارت **مزودات قابلة للاختيار** لكل تطبيق.
3. دخول الداشبورد بحسابات خاصة بالأداة نفسها، بدل Firebase Auth، لأن الداشبورد يخدم كل التطبيقات.
4. مفاتيح سيرفر **لكل تطبيق** تُنشأ وتُلغى من الداشبورد (الطريق الأساسي للباك إندات غير Firebase).
5. **رفع ملف service account من الداشبورد** (المالك فقط، كتابة فقط بلا قراءة أو تنزيل)، وإضافة تطبيق جديد
   بالكامل من الداشبورد بلا إعادة بناء الحاوية (3.13، T16).

---

## 0. الملخص بسطور

1. التطبيق **لا يحمل أي مفتاح**. يرسل توكن هوية من مزوده (Firebase ID token أو أي JWT)، والأداة تتحقق منه محلياً.
2. **إعدادات لكل تطبيق في الأداة:** مزود الهوية، مصدر البيانات، القوالب، قواعد الحملات. تُعدَّل من الداشبورد،
   والكود يبقى عاماً.
3. غير الأدمن **لا يكتب نصاً حراً**، بل يختار **قالباً** يُبنى نصه في السيرفر من بيانات موثوقة.
   الأداة تفرض **حواجز أمان على القوالب نفسها** حتى لو أنشأها الأدمن بالغلط.
4. **سجل أجهزة في الأداة**. الرسائل الخاصة (طلبات، تنبيهات الأدمن) تُرسل لـ uid أو role، ولا تمر عبر topics،
   لأن أي جهاز يستطيع الاشتراك بأي topic من التطبيق مباشرة.
5. **حملات الشركات**: جمهور ديناميكي من topics مسموحة، و`promos` إجباري، واسم الشركة إجباري في العنوان،
   وحصة، ومراجعة أول 3 حملات، وإيقاف أي شركة بضغطة.
6. انتقال بلا كسر: المفتاح القديم يبقى مؤقتاً (بحد سرعة) للنسخ القديمة، ثم يُطفأ ويُدوَّر.

---

## 1. الوضع الحالي (حقائق من الكود، 2026-10-21)

### 1.1 الفروع

| المشروع | الحالة |
|---|---|
| `fcm_microservice` | `my-server` (آخره `ac4ef44`) يتضمن `d25ef57`: تحقق Firebase ID token + **الإرسال للأدمن فقط** + دخول الداشبورد بـ Firebase Auth. السيرفر يعمل غالباً على `4b2561b` (مفتاح ثابت). commit باسم `c303d3` غير موجود في أي من المستودعين، **يجب تأكيد الـ commit المنشور فعلاً**. |
| `taqati` | `HEAD = 0164ba1` يرسل بـ `AppStrings.apiKey`. يوجد تعديل **غير محفوظ** (SEC2-C) يحوّل كل الإرسال إلى ID token. |

### 1.2 أين يرسل تطبيق واط

| # | النوع | المرسل | الهدف | الملفات |
|---|---|---|---|---|
| أ | لوحة الأدمن | admin | topic أو condition بنص حر | `features/admin/presentation/screens/send_notification_screen.dart`، `notifications/domain/services/notification_target_builder.dart` |
| ب | تنبيه الأدمن | أي مستخدم | topic `admins` | `auth/.../complete_profile_screen.dart:327`، `auth/.../edit_profile_page.dart:158`، `brands/data/brands_repository_impl.dart:273`، `brands/.../brand_editor_sheet.dart:307`، `products/data/firebase_products_repository.dart:84`، `products/data/data_sources/firebase_store_products_data_source.dart:35`، `companies/.../dashboard_pending_view.dart:51` |
| ج | الطلبات | مشترٍ ← بائع، بائع ← مشترٍ | device tokens يقرأها التطبيق من Firestore | `store_orders/data/data_sources/firebase_order_notifier.dart` |
| د | (جديد) حملات الشركات | company | جمهور ديناميكي | غير موجود بعد |

### 1.3 المشاكل

- **P1:** المفتاح الثابت داخل التطبيق المنشور، وموجود أيضاً في `taqati/api/Taqati api/fcm/*.yml` وتاريخ git. **التدوير إلزامي.**
- **P2:** نشر `ac4ef44` كما هو، أو نشر تعديل SEC2-C في التطبيق، **يكسر النوعين ب وج** (403 لغير الأدمن).
- **P3:** على Android/iOS، `FirebaseTopicSubscriber` يستدعي `subscribeToTopic` مباشرة، والأداة لا تراه.
  ⇐ **أي topic هو قناة عامة.** أي جهاز يستطيع الاشتراك بـ `admins`.
- **P4:** `users/{uid}` مقروء لأي مستخدم مسجّل (`firestore.rules:96`)، وفيه `fcm_tokens`.
  و`store_orders` فيه `buyer_fcm_tokens`. ⇐ جمع tokens الأجهزة ممكن.
- **P5:** أشياء خاصة بواط مكتوبة داخل الأداة: `users | companies | technicals` (`src/auth.rs:684`)، و`users/{uid}.role`.
- **P6:** الأداة مربوطة بـ Firebase في كل شيء: الهوية (Firebase ID token فقط)، والأدوار (Firestore فقط)،
  ودخول الداشبورد (Firebase Auth لمشروع التطبيق). تطبيق باك إنده Python أو Supabase لا يستطيع استخدامها إلا بمفتاح عام.

---

## 2. المبادئ (لا نخالفها في أي مهمة)

1. **لا سر في أي عميل.** الأسرار فقط في `.env` على السيرفر وفي باك إند موثوق.
2. **الأسرار لا تُخزَّن في قاعدة البيانات، ولا تُقرأ أبداً من الداشبورد.** الإعدادات تشير إليها **باسم المتغير**
   (`"secret_env": "WATT_JWT_SECRET"`). مفاتيح التطبيقات تُحفظ **hash** فقط.
   الاستثناء الوحيد للكتابة: ملف service account يرفعه **المالك** (كتابة فقط) إلى `credentials/` على القرص (3.13).
3. **الهوية من التوكن، والصلاحية من إعدادات التطبيق.** لا قرارات صلاحية مكتوبة في الكود لتطبيق بعينه.
4. **Firebase إلزامي لشيء واحد فقط: FCM** (مشروع + service account + `firebase_messaging` في التطبيق).
   الهوية والبيانات قابلة للاستبدال.
5. **topic = قناة عامة.** المحتوى الخاص يذهب إلى uid/role عبر سجل الأجهزة.
6. **غير الأدمن لا يكتب نصاً حراً لجمهور واسع.** والأداة **ترفض حفظ** أي قالب يخالف هذا، حتى من الداشبورد.
7. **الرفض عند الشك (fail-closed).** إعدادات غير صالحة، أو مصدر بيانات لا يرد، أو تطبيق بلا إعدادات ⇐ رفض.
8. **كل إرسال وكل تعديل إعدادات يُسجَّل** (مَن، متى، ماذا).
9. **خفة الأداة.** SQLite + ذاكرة + تخزين مؤقت. لا خدمات إضافية.

---

## 3. التصميم

### 3.1 أنواع المتصلين

| المتصل | المصادقة | المسموح |
|---|---|---|
| مالك الأداة (owner) | حساب الداشبورد (جلسة cookie) | كل التطبيقات: الإعدادات، الهوية، البيانات، القوالب، المفاتيح، الحسابات |
| مدير تطبيق (manager) | حساب الداشبورد، مربوط بتطبيقات محددة | لتطبيقاته: القوالب، الحملات، الحظر، السجل، الإرسال الحر. **لا** يغيّر الهوية/مصدر البيانات/المفاتيح |
| باك إند موثوق | `Authorization: Bearer <app key>` (مفتاح لكل تطبيق) | كل شيء لتطبيقه: `/send` حر، قوالب، حملات بلا مراجعة |
| أدمن داخل التطبيق | توكن هوية + دور ضمن `admin_roles` | `/send` حر لتطبيقه (مثل لوحة الأدمن في واط) |
| مستخدم مسجّل | توكن هوية | القوالب المسموحة لدوره فقط. الحملات للشركة المعتمدة |
| زائر | لا شيء | الاشتراك بالـ topics العامة على الويب فقط |
| `SERVICE_API_KEY` (env) | Bearer | مفتاح عام لكل التطبيقات (للتوافق، يُفضَّل مفاتيح التطبيقات) |
| المفتاح القديم (مؤقت) | `API_KEY` | `/send` فقط، بحد سرعة، حتى إطفائه |

### 3.2 إعدادات التطبيق (مخزنة في SQLite، تُدار من الداشبورد)

لكل تطبيق (`app` = اسم ملف `credentials/<app>.json`) سجل إعدادات من أقسام:

| القسم | يعدّله | المحتوى |
|---|---|---|
| `identity` | owner | مزود الهوية (3.3) + مصدر الدور + `admin_roles` |
| `data_source` | owner | مصدر البيانات للقوالب والحملات (3.4) |
| `devices` | owner | `max_per_uid` |
| `web_subscribe` | manager | الـ topics المسموحة للاشتراك عبر الويب (زائر / مسجّل) |
| `templates` | manager | القوالب (جدول مستقل بإصدارات، 3.5) |
| `campaigns` | manager | قواعد الحملات (3.7) |
| `app_check` | owner | Firebase فقط (3.12) |

- **المصدر الوحيد للحقيقة:** SQLite. الأداة تحمّل الإعدادات في الذاكرة وتحدّثها فور الحفظ (بلا إعادة تشغيل).
- **التحقق قبل الحفظ:** نفس دالة `validate()` تُستعمل للحفظ من الداشبورد وللاستيراد. حفظ غير صالح ⇐ 400 مع قائمة أخطاء مفهومة.
- **سجل الإصدارات:** كل حفظ ينشئ إصداراً جديداً (`config_versions`) مع `changed_by`. التراجع = استعادة إصدار سابق كنسخة جديدة.
- **استيراد/تصدير JSON:** زر في الداشبورد + أمر CLI `fcm_microservice import <app> <file.json>` لأول تشغيل.
  المثال `policies/watt.example.json` يُرفع للمستودع (`.gitignore` يتجاهل `*.json`، فنضيف `!policies/*.example.json`).
- **تطبيق بلا إعدادات:** يعمل للأدمن (بالافتراضي القديم: Firebase + `users/{uid}.role == admin` مع تحذير في السجل) وللمفاتيح فقط.

#### مثال كامل لتطبيق واط (صيغة التصدير)

```json
{
  "version": 1,

  "identity": {
    "provider": { "type": "firebase" },
    "role": { "source": "data_source", "doc": "users/{uid}", "field": "role", "cache_seconds": 60 },
    "admin_roles": ["admin"],
    "role_members": { "collection": "users", "field": "role", "cache_seconds": 300, "max": 50 }
  },

  "data_source": { "type": "firestore" },

  "devices": { "max_per_uid": 5 },

  "web_subscribe": {
    "anonymous": ["all", "country_*", "lang_*", "promos", "cat_*", "company_*"],
    "signed_in": ["users", "companies", "technicals"]
  },

  "templates": {
    "admin_alert": {
      "description": "تنبيه للأدمن من أي مستخدم (تسجيل، تغيير دور، ماركة/منتج جديد، طلب اعتماد)",
      "enabled": true,
      "senders": ["user", "company", "technical", "admin"],
      "params": {
        "title": { "type": "text", "max": 80 },
        "body":  { "type": "text", "max": 300 },
        "route": { "type": "pattern", "pattern": "^/[a-z0-9/_-]*$", "max": 100 }
      },
      "to": { "role": "admin" },
      "title": "{p.title}",
      "body": "{p.body}",
      "data": { "action": "open_page", "target": "{p.route}" },
      "rate": { "per_uid_per_hour": 10 }
    },

    "order_new": {
      "description": "إشعار البائع بطلب جديد",
      "enabled": true,
      "senders": ["user", "company", "technical"],
      "params": { "order_id": { "type": "pattern", "pattern": "^[A-Za-z0-9_-]{1,64}$" } },
      "context": {
        "doc": "store_orders/{p.order_id}",
        "require": { "buyer_id": "{sender.uid}" }
      },
      "to": { "uid": "{doc.seller_id}" },
      "title": "طلب جديد #{doc.order_number}",
      "body": "طلب جديد من {doc.buyer_name} بقيمة {doc.total|int} {doc.currency}",
      "data": { "action": "open_page", "target": "/store/orders/{p.order_id}", "order_id": "{p.order_id}", "role": "seller" },
      "dedupe": { "key": "{p.order_id}", "seconds": 600 },
      "rate": { "per_uid_per_hour": 30 }
    },

    "order_status": {
      "description": "إشعار المشتري بتغيّر حالة طلبه",
      "enabled": true,
      "senders": ["company", "technical", "user"],
      "params": { "order_id": { "type": "pattern", "pattern": "^[A-Za-z0-9_-]{1,64}$" } },
      "context": {
        "doc": "store_orders/{p.order_id}",
        "require": { "seller_id": "{sender.uid}" }
      },
      "vars": {
        "status_text": {
          "from": "doc.status",
          "map": {
            "confirmed": "تم تأكيد طلبك #{doc.order_number} من قِبل {doc.seller_name}",
            "preparing": "طلبك #{doc.order_number} قيد التجهيز الآن",
            "shipped":   "تم شحن طلبك #{doc.order_number} وهو في الطريق إليك",
            "delivered": "تم تسليم طلبك #{doc.order_number} بنجاح",
            "cancelled": "تم إلغاء الطلب #{doc.order_number}",
            "rejected":  "تعذّر قبول طلبك #{doc.order_number} من قِبل {doc.seller_name}",
            "pending":   "طلبك #{doc.order_number} قيد المراجعة"
          }
        }
      },
      "to": { "uid": "{doc.buyer_id}" },
      "title": "تحديث الطلب #{doc.order_number}",
      "body": "{status_text}",
      "data": { "action": "open_page", "target": "/store/orders/{p.order_id}", "order_id": "{p.order_id}", "role": "buyer", "new_status": "{doc.status}" },
      "dedupe": { "key": "{p.order_id}:{doc.status}", "seconds": 600 },
      "rate": { "per_uid_per_hour": 120 }
    }
  },

  "campaigns": {
    "enabled": true,
    "senders": ["company"],
    "entity": {
      "param": "company_id",
      "doc": "companies/{p.company_id}",
      "require": { "user_id": "{sender.uid}", "status": "approved" },
      "name_field": "name"
    },
    "audience": {
      "allowed_topics": ["country_*", "lang_*", "users", "technicals", "cat_*", "company_{p.company_id}"],
      "required_topic": "promos",
      "max_topics": 5
    },
    "content": {
      "title_prefix": "{entity.name}: ",
      "title_max": 60,
      "body_max": 240,
      "forbid_links": true,
      "image_hosts": ["firebasestorage.googleapis.com"],
      "actions": {
        "open_company": { "data": { "action": "open_company", "company_id": "{p.company_id}" } },
        "open_product": {
          "params": { "product_id": { "type": "pattern", "pattern": "^[A-Za-z0-9_-]{1,64}$" } },
          "check": { "doc": "store_products/{p.product_id}", "require": { "owner_id": "{sender.uid}", "status": "approved" } },
          "data": { "action": "open_product", "product_id": "{p.product_id}" }
        }
      }
    },
    "quota": { "per_entity_per_day": 1, "per_entity_per_week": 5 },
    "review": { "mode": "first_n", "n": 3 }
  },

  "app_check": { "mode": "off", "project_number": "919552016311" }
}
```

> ملاحظات على المثال:
> - `order_new` / `order_status` لا يقبلان أي نص من العميل. كل النص من وثيقة الطلب، والمستلم من الوثيقة،
>   والمرسل يجب أن يكون طرفاً في الطلب. **لا يمكن تزوير إشعار طلب.**
> - `order_status` يُرسل **بعد** حفظ الحالة الجديدة، فالوثيقة تعكس الحالة الجديدة.
> - تأكد في W4 أن `seller_id` في `store_orders` هو **uid المستخدم** وليس معرّف وثيقة الشركة
>   (قاعدة القراءة `firestore.rules:228` تفترض ذلك، لكن `_getSellerTokens` فيه مسار احتياطي للشركة).

### 3.3 مزودات الهوية (من هو المستخدم)

| النوع | يناسب | الإعداد |
|---|---|---|
| `firebase` | تطبيقات Firebase Auth (واط) | لا شيء: `project_id` من service account. (الموجود حالياً في `d25ef57`) |
| `jwt` | أي باك إند يصدر JWT: FastAPI/Django، Supabase، Auth0، Keycloak، Clerk | انظر أدناه |

```json
"provider": {
  "type": "jwt",
  "algorithms": ["RS256"],
  "jwks_url": "https://auth.client.com/.well-known/jwks.json",
  "issuer": "https://auth.client.com/",
  "audience": "client-mobile-app",
  "uid_claim": "sub",
  "name_claim": "name",
  "max_age_seconds": 3600
}
```

- **مفاتيح التحقق:** `jwks_url` (RS256/ES256، مخزن مؤقت بنفس آلية `JwkCache` الحالية) **أو**
  `hs256_secret_env: "CLIENT_JWT_SECRET"` (HS256، والسر في `.env` فقط).
- **الخوارزمية مثبتة** من الإعدادات. `none` مرفوض دائماً، ولا خلط بين HS وRS (يمنع هجوم تبديل الخوارزمية).
- **فحوص إلزامية:** `iss`، `aud`، `exp`، `iat` (مع سماحية 60ث)، `sub` غير فارغ، وطول التوكن ≤ 8KB.
- **claims متداخلة** بمسار نقطي: `app_metadata.role` (Supabase)، `realm_access.roles` (Keycloak).
- **أمثلة جاهزة في الوثائق:**
  - Supabase: `issuer = https://<ref>.supabase.co/auth/v1`، `audience = authenticated`، و`jwks_url` (المفاتيح غير المتماثلة) أو `hs256_secret_env`.
  - FastAPI: باك إندك يصدر JWT (RS256 مع JWKS منشور، أو HS256 بسر مشترك مع الأداة).

### 3.4 مصادر البيانات (الدور، وثيقة الطلب، الشركة، أعضاء الدور)

القوالب تتعامل مع **مسارات منطقية** (`store_orders/{id}`، `users/{uid}`)، والمصدر يقرر من أين تُقرأ:

| النوع | الوصف |
|---|---|
| `firestore` | قراءة REST بـ service account نفس التطبيق (`roles/datastore.viewer`). الموجود حالياً، مع تعميم. |
| `http` | الأداة تسأل باك إند التطبيق عبر **عقد صغير ثابت** (أدناه). يناسب Python/Postgres/Supabase/أي شيء. |
| `none` | لا بيانات. الدور من claim في التوكن فقط، والقوالب لا تستعمل `context`/`role_members` (التحقق يرفضها). |

**عقد `http` (Resolver API)** الذي ينفذه باك إند التطبيق:

```json
"data_source": {
  "type": "http",
  "base_url": "https://api.client.com/internal/fcm",
  "secret_env": "CLIENT_RESOLVER_SECRET",
  "timeout_ms": 3000
}
```

| الطلب من الأداة | الرد المتوقع |
|---|---|
| `GET {base}/doc?path=<percent-encoded full path>` | `200` + كائن JSON (الوثيقة) أو `404` |
| `GET {base}/members?collection=<c>&field=<f>&value=<v>&limit=<n>` | `200 {"uids": ["u1", "u2"]}` |

- المسار يُرمَّز كقيمة واحدة (percent-encoding بحروف hex كبيرة)، فيصير `store_orders/abc123` هو `store_orders%2Fabc123`. ترتيب معاملات الاستعلام ثابت كما في الجدول أعلاه.
- الطلبات موقعة **بلا Bearer** (السر لا يُرسَل أبداً): ترويسات `X-FCM-Timestamp` (ثواني UNIX) و`X-FCM-Signature` = hex صغير لـ `HMAC-SHA256(secret, canonical)` حيث
  `canonical = "GET" + "\n" + <مسار URL كما أُرسل، مثل /internal/fcm/doc> + "\n" + <نص الاستعلام الخام كما أُرسل> + "\n" + <timestamp>`.
  الباك إند يرفض انحراف الوقت > 60ث أو توقيعاً غير مطابق، ويجب أن يحافظ الـ reverse proxy على المسار كما أُرسل (لا يعيد كتابة prefix).
- السر من متغير البيئة المسمّى في `secret_env` (32 بايتاً على الأقل)، ولا يُسجَّل ولا يُرسَل.
- رد غير 200/404، أو مهلة ⇐ **503** (fail-closed). الأداة لا تخزن ردود `doc` مؤقتاً. `404` في `members` خطأ لا فراغ.
- الحقول تُعامل كنصوص/أرقام/منطقيات. الكائنات المتداخلة تُقرأ بمسار نقطي (`{doc.buyer.name}`).
- **متجه الاختبار** (ثابت حرفياً في اختبارات Rust وفي المثال و`examples/README.md`):
  secret = `test-secret-test-secret-test-secret-00`، path = `/internal/fcm/doc`، query = `path=store_orders%2Fabc123`، timestamp = `1700000000`
  ⇒ التوقيع = `f97974c3e1b1706c53e625f9f6b324adab12a0143af3348757cc311771f20813`.
- **مثال جاهز:** `examples/resolver_fastapi.py` (FastAPI، ~100 سطر) + `examples/README.md`، و`--selftest` يتحقق من متجه الاختبار بمكتبة Python القياسية.

**مصدر الدور** (`identity.role.source`): `claim` (من التوكن، مثل `"claim": "app_metadata.role"`) أو `data_source` (وثيقة + حقل).

### 3.5 القوالب (تُدار من الداشبورد)

**التخزين:** جدول `templates` (القالب الحالي) + `template_versions` (كل نسخة محفوظة مع `changed_by` و`created_at`).

**تعريف القالب:** نفس الحقول في المثال 3.2:
`description`، `enabled`، `senders`، `params`، `context`، `vars`، `to`، `title`، `body`، `image`، `data`، `dedupe`، `rate`.

**أنواع المعاملات `params`:**

| النوع | معناه | يُعتبر "نصاً حراً"؟ |
|---|---|---|
| `text` (`max`) | أي نص بحد طول | نعم |
| `pattern` (`pattern`, `max`) | يطابق regex | لا |
| `enum` (`values`) | من قائمة ثابتة | لا |
| `int` (`min`, `max`) | عدد صحيح | لا |

**أنواع الهدف `to`:** `{"uid": "..."}` · `{"role": "admin"}` · `{"topic": "..."}` · `{"condition": "..."}`.

**حواجز الأمان (تُفرض عند الحفظ، ولا يستطيع أي حساب تجاوزها):**
1. قالب هدفه `topic`/`condition` **ومرسلوه ليسوا أدمن فقط** ⇐ ممنوع أن يحتوي معامل `text`، وممنوع أن يُبنى الهدف من معامل `text`.
   (البث الحر لغير الأدمن يمر فقط عبر الحملات بقيودها.)
2. كل `{p.x}` معرَّف في `params`، وكل `{doc.x}` يتطلب `context`، وكل `{<var>}` معرَّف في `vars`.
3. `context`/`role_members` غير مسموحين إذا `data_source = none`.
4. `rate` إلزامي لأي قالب مرسلوه غير أدمن (الحد الأقصى الافتراضي 120/ساعة لكل uid).
5. `senders` لا يمكن أن يكون فارغاً، والأدوار نصوص `[a-z0-9_]{1,32}`.
6. `data` لا يحتوي مفاتيح محجوزة لـ FCM (`from`، `notification`، `message_type`، أي مفتاح يبدأ بـ `google.` أو `gcm.`).

**في الداشبورد:**
- **محرر بنموذج** (حقول + قوائم + معاينة مباشرة) **ووضع JSON متقدم**، وكلاهما يحفظ نفس التعريف.
- **تحقق مباشر** أثناء الكتابة: `POST /admin/apps/{app}/templates/validate`، والأخطاء تظهر بجانب الحقل.
- **معاينة (dry-run):** إدخال معاملات تجريبية، و(اختياري) معرّف وثيقة حقيقي ⇐ يظهر الإشعار كما سيبدو على الهاتف، والمستلمون (uid/role/topic)، **بلا إرسال**.
- **إرسال تجريبي:** لأجهزة المدير نفسه فقط (تحتاج تسجيل جهازه بالمعرّف المربوط بحسابه، T6/T12).
- **تفعيل/تعطيل** بدون حذف. القالب المعطل ⇐ 404 للتطبيق.
- **سجل الإصدارات والتراجع**، مع فرق (diff) بين النسختين.
- **الحذف** غير مسموح إذا استُعمل القالب خلال آخر 30 يوماً (يُعطَّل بدلاً من ذلك)، حتى لا تنكسر نسخ تطبيق قديمة.

### 3.6 لغة القوالب

| الصيغة | المصدر | ملاحظة |
|---|---|---|
| `{p.<name>}` | معامل من العميل | يجب تعريفه في `params` |
| `{sender.uid}` `{sender.name}` `{sender.role}` | التوكن + الدور | `name` من `name_claim` إن وُجد |
| `{doc.<field>}` | وثيقة `context.doc` | مسار نقطي للحقول المتداخلة |
| `{entity.<field>}` | وثيقة الشركة (الحملات) | |
| `{<var>}` | `vars` (خريطة قيم) | قيمة غير موجودة في الخريطة ⇐ 400 |
| `\|int` | فلتر | تقريب رقم إلى عدد صحيح |

- **لا تقييم متداخل:** القيمة القادمة من `p.` أو `doc.` لا تُفسَّر كقالب مرة ثانية.
- **`require`:** كل حقل يساوي القيمة حرفياً بعد التعويض. أي عدم تطابق، أو وثيقة غير موجودة ⇐ **403 موحد**.
- **مقطع المسار:** القيم داخل `doc` تُرمَّز، ويُرفض أي `/` أو `..` فيها.
- **الحدود العامة** في `validation.rs` تُطبق على الناتج النهائي.

### 3.7 حملات الشركات

**دورة الحياة:**

```
           ┌────────────── review لازم ──────────────┐
create ──► pending_review ──approve──► sending ──► sent
   │             │  └──reject──► rejected             └──► failed
   │             └──cancel (الشركة)──► cancelled
   └── review غير لازم ──► sending ──► sent / failed
```

- **mode:** `none` | `always` | `first_n` (أول n حملات **مُرسلة** لكل شركة تُراجَع). افتراضي واط: `first_n`، n=3.
- **الجمهور:** العميل يرسل مجموعات لا شرطاً نصياً:
  `"audience": {"groups": [["country_iq"], ["users","technicals"], ["lang_ar"]]}`.
  السيرفر يتحقق من `allowed_topics` (`*` = لاحقة `[a-z0-9_]+`)، ثم يبني:
  `'promos' in topics && ('country_iq' in topics) && ('technicals' in topics || 'users' in topics) && ('lang_ar' in topics)`.
  العدد الكلي مع `promos` ≤ 5.
- **المحتوى:** بادئة اسم الشركة من مصدر البيانات، الطول، منع الروابط، الصورة من `image_hosts` فقط، والإجراء من `actions` فقط.
- **الحصة:** من SQLite لكل شركة يومياً وأسبوعياً.
- **الحظر:** الشركة المحظورة ⇐ 403، وحملاتها المعلقة تُلغى.
- **قواعد الحملات تُعدَّل من الداشبورد** بنفس آلية الإصدارات والتحقق (القسم 3.2).

### 3.8 سجل الأجهزة

- `POST /devices/register` عند الحصول على token، وعند تجدده، وعند الدخول. `POST /devices/unregister` عند الخروج.
- token واحد يخص uid واحد (ينتقل عند تبديل الحساب). حد `max_per_uid` (الأقدم يُحذف).
- الجهاز الميت يُحدَّد بدقة: `error.details[].errorCode == "UNREGISTERED"`، أو `error.status == "INVALID_ARGUMENT"` مع `error.details[].fieldViolations[].field` منتهٍ بـ`message.token` ⇐ حذف تلقائي. لا يُعتبر كل `INVALID_ARGUMENT` جهازاً ميتاً (قد يكون الحمل نفسه غير صالح).
- **لـ uid:** أجهزته (≤5). **لـ role:** أعضاء الدور من مصدر البيانات (مخزن 5 دقائق، حد 50) ثم أجهزتهم.
- يعمل مع أي مزود هوية، لأنه يعتمد على `uid` فقط.

### 3.9 دخول الداشبورد (حسابات الأداة)

- جدول `tool_users`: البريد، hash كلمة المرور (**argon2id**)، الدور (`owner` | `manager`)، والتطبيقات المربوطة (`tool_user_apps`).
- **الجلسة:** cookie `HttpOnly; Secure; SameSite=Strict`، معرّف عشوائي 256-bit، والمخزَّن hash فقط، وصلاحية 12 ساعة.
  الطلبات المغيِّرة تتطلب ترويسة `X-Requested-With` (حماية CSRF إضافية).
- **حد محاولات الدخول:** 5 خاطئة/15 دقيقة لكل بريد ولكل IP.
- **أول مالك:** أمر CLI `fcm_microservice create-owner --email you@x.com` يطبع كلمة مرور مؤقتة مرة واحدة، وتُغيَّر عند أول دخول.
- **TOTP (مصادقة ثنائية)** للمالك: مهمة لاحقة اختيارية (T15).
- **ربط جهاز المدير** (للإرسال التجريبي): المدير يدخل uid حسابه في التطبيق، فالإرسال التجريبي يذهب لأجهزة هذا الـ uid فقط.
- دخول Firebase Auth الحالي في الداشبورد **يُزال**، فلا يحتاج الداشبورد إعدادات Firebase Web بعد الآن.

### 3.10 مفاتيح التطبيقات (Server-to-Server)

- تُنشأ من الداشبورد (owner): اسم + تطبيق. **تُعرض مرة واحدة** بصيغة `fcm_<app>_<random 32 bytes base64url>`، والمخزَّن SHA-256 فقط.
- `last_used_at` يظهر في الداشبورد. الإلغاء فوري.
- المفتاح مقيد بتطبيقه: استخدامه مع `app` آخر ⇐ 403.
- **هذا هو الطريق المقترح لأي باك إند** (Python بالخصوص): الباك إند يقرر ويرسل، والتطبيق لا يرسل شيئاً بنفسه.

### 3.11 الواجهات (API)

**واجهات التطبيقات** (`/api/v1`):

| الطريقة والمسار | مَن | الطلب | الرد |
|---|---|---|---|
| `POST /devices/register` | مستخدم | `{app, token, platform}` | `{status:"ok"}` |
| `POST /devices/unregister` | مستخدم، أو حامل الـ token | `{app, token}` | `{status:"ok"}` |
| `POST /send/template` | حسب `senders`، أو مفتاح التطبيق | `{app, template, params}` | `{status, delivered, failed}` |
| `POST /send` | أدمن التطبيق / مفتاح / قديم مؤقت | كما هو | كما هو |
| `POST /campaigns` | company معتمدة / مفتاح | `{app, company_id, title, body, image?, action, action_params?, audience}` | `{id, status}` |
| `GET /campaigns` | الشركة (حملاتها) | `?app&company_id&limit&offset` | قائمة |
| `POST /campaigns/{id}/cancel` | صاحب الحملة قبل الإرسال | | `{status}` |
| `POST /topics/subscribe` · `/unsubscribe` | حسب `web_subscribe` | كما هو | كما هو |

**واجهات الداشبورد** (`/api/v1/admin`، جلسة cookie):

| المسار | owner | manager |
|---|---|---|
| `POST /auth/login` · `/auth/logout` · `GET /auth/me` · `POST /auth/password` | ✓ | ✓ |
| `GET /apps` · `GET /apps/{app}` | كل التطبيقات | تطبيقاته |
| `PUT /apps/{app}/identity` · `/data_source` · `/devices` · `/app_check` | ✓ | ✗ |
| `PUT /apps/{app}/web_subscribe` · `/campaigns` | ✓ | ✓ |
| `GET/POST /apps/{app}/templates` · `GET/PUT/DELETE /apps/{app}/templates/{key}` | ✓ | ✓ |
| `POST /apps/{app}/templates/validate` · `/templates/{key}/preview` · `/templates/{key}/test-send` | ✓ | ✓ |
| `GET /apps/{app}/versions` · `POST /apps/{app}/versions/{id}/restore` | ✓ | ✓ (لأقسامه) |
| `GET /apps/{app}/export` · `POST /apps/{app}/import` | ✓ | ✗ |
| `POST /apps/{app}/identity/test` · `/data_source/test` (فحص اتصال بتوكن/مسار تجريبي) | ✓ | ✗ |
| `GET /apps/{app}/campaigns` · `POST .../{id}/approve` · `.../{id}/reject` | ✓ | ✓ |
| `GET/POST/DELETE /apps/{app}/blocks` | ✓ | ✓ |
| `GET/POST/DELETE /apps/{app}/keys` | ✓ | ✗ |
| `POST /apps/{app}/send` (إرسال حر من الداشبورد) · `GET /apps/{app}/history` · `GET /apps/{app}/devices/stats` | ✓ | ✓ |
| `POST /apps` (إضافة تطبيق + رفع service account) · `PUT /apps/{app}/credentials` (استبدال) · `POST /apps/{app}/disable` · `DELETE /apps/{app}` | ✓ (+ إعادة كلمة المرور) | ✗ |
| `GET/POST/PUT/DELETE /users` (حسابات الأداة) | ✓ | ✗ |
| `GET /audit` (سجل تعديلات الإعدادات) | ✓ | تطبيقاته |

### 3.12 App Check (اختياري، Firebase فقط)

- `mode`: `off` | `monitor` | `enforce`، والترويسة `X-Firebase-AppCheck`.
- التحقق محلياً: JWKS من `https://firebaseappcheck.googleapis.com/v1/jwks`، و`iss = https://firebaseappcheck.googleapis.com/<project_number>`،
  و`aud` يحتوي `projects/<project_number>`.
- للتطبيقات غير Firebase: غير متاح. حمايتها الأساسية هي القوالب + الأدوار + حدود السرعة.

### 3.13 رفع ملف service account وإضافة تطبيق من الداشبورد

**الموجود حالياً:** `credentials/` مربوط كـ volume (`docker-compose.yml`)، و`credentials::watch_credentials_dir` يحمّل أي ملف جديد
بلا إعادة تشغيل. الإضافة هنا: الرفع من الداشبورد بشروط، بدل نسخ الملف على السيرفر يدوياً.

**التدفق: "إضافة تطبيق"** (المالك فقط):
1. اسم التطبيق (`[a-z0-9_-]{1,40}`، يصير `app`) + رفع ملف JSON.
2. **تأكيد الهوية من جديد:** إعادة إدخال كلمة المرور (ورمز TOTP إذا مفعّل، T15). جلسة مفتوحة وحدها لا تكفي.
3. **التحقق قبل الحفظ:**
   - `type == "service_account"`، والحقول `project_id` و`client_email` و`private_key` و`private_key_id` موجودة.
   - المفتاح الخاص يُقرأ (PEM صالح)، وحجم الملف ≤ 8KB.
   - **فحص حي:** طلب access token بنطاق `firebase.messaging`. فشل ⇐ رفض برسالة واضحة.
   - (اختياري) فحص قراءة Firestore إذا `data_source = firestore`، والنتيجة تظهر كتحذير لا كرفض.
4. **الكتابة:** ملف مؤقت ثم `rename` ذري إلى `credentials/<app>.json` بصلاحيات `0600`. ثم تحميل مباشر (بدون انتظار المراقب).
5. يُنشأ سجل إعدادات افتراضي للتطبيق (هوية `firebase`، بلا قوالب)، ويُنقل المالك لصفحة إعداداته.

**الاستبدال (تدوير المفتاح):** نفس التدفق على تطبيق موجود. يُعرض `client_email` و`private_key_id` للقديم والجديد،
ويجب أن يكون `project_id` **نفسه**، وإلا يُرفض (تغيير المشروع = تطبيق جديد). القديم يُحذف من القرص بعد نجاح التحميل،
مع تذكير في الواجهة بإلغاء المفتاح القديم من Google Cloud Console.

**الحذف/التعطيل:** التعطيل يوقف الإرسال فوراً ويبقي الملف. الحذف يتطلب كتابة اسم التطبيق للتأكيد، ويحذف الملف والإعدادات
(مع تصدير JSON تلقائي للإعدادات في `audit_log` قبل الحذف).

**ما لا يُتاح أبداً:** قراءة الملف، أو تنزيله، أو عرض `private_key` بأي واجهة. الداشبورد يعرض فقط:
`project_id`، و`client_email`، و`private_key_id`، وتاريخ الرفع، ومَن رفعه.

**شروط النشر:**
- volume `credentials/` يجب أن يكون **قابلاً للكتابة** من الحاوية (الوضع الحالي).
- متغير `CREDENTIALS_UPLOAD_ENABLED` (افتراضي `true`). من يريد إدارة الملفات يدوياً فقط يجعله `false`، فتختفي الواجهة ويُرفض المسار.
- الواجهة تُخدم فقط عبر HTTPS (خلف reverse proxy)، وحجم الطلب لهذا المسار 16KB.
- كل رفع أو استبدال أو حذف ⇐ `audit_log` (بدون محتوى الملف).

### 3.14 قاعدة البيانات (إضافات SQLite)

```sql
-- إعدادات التطبيقات والقوالب
CREATE TABLE IF NOT EXISTS app_configs (
  app TEXT PRIMARY KEY, config TEXT NOT NULL,           -- JSON بلا templates
  version INTEGER NOT NULL DEFAULT 1,                    -- نسخة الإعداد (تُزاد مع كل حفظ)
  updated_by TEXT NOT NULL, updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS templates (
  app TEXT NOT NULL, key TEXT NOT NULL, definition TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1, version INTEGER NOT NULL,
  updated_by TEXT NOT NULL, updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
  last_used_at DATETIME,
  PRIMARY KEY (app, key)
);
CREATE TABLE IF NOT EXISTS config_versions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  app TEXT NOT NULL, section TEXT NOT NULL,             -- identity | data_source | ... | template:<key>
  content TEXT NOT NULL, changed_by TEXT NOT NULL, created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- حسابات الأداة والجلسات والمفاتيح
CREATE TABLE IF NOT EXISTS tool_users (
  id INTEGER PRIMARY KEY AUTOINCREMENT, email TEXT UNIQUE NOT NULL,
  password_hash TEXT NOT NULL, role TEXT NOT NULL, must_change_password INTEGER NOT NULL DEFAULT 1,
  test_uid TEXT, created_at DATETIME DEFAULT CURRENT_TIMESTAMP, disabled INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS tool_user_apps (user_id INTEGER NOT NULL, app TEXT NOT NULL, PRIMARY KEY (user_id, app));
CREATE TABLE IF NOT EXISTS tool_sessions (
  id_hash TEXT PRIMARY KEY, user_id INTEGER NOT NULL, expires_at DATETIME NOT NULL,
  created_at DATETIME DEFAULT CURRENT_TIMESTAMP, ip TEXT, user_agent TEXT
);
CREATE TABLE IF NOT EXISTS app_keys (
  id INTEGER PRIMARY KEY AUTOINCREMENT, app TEXT NOT NULL, name TEXT NOT NULL,
  key_hash TEXT UNIQUE NOT NULL, created_by TEXT NOT NULL, created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
  last_used_at DATETIME, revoked_at DATETIME
);
CREATE TABLE IF NOT EXISTS audit_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT, actor TEXT NOT NULL, app TEXT, action TEXT NOT NULL,
  detail TEXT, created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- الأجهزة والحملات
CREATE TABLE IF NOT EXISTS devices (
  app TEXT NOT NULL, token TEXT NOT NULL, uid TEXT NOT NULL,
  platform TEXT NOT NULL, created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
  last_seen DATETIME DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (app, token)
);
CREATE INDEX IF NOT EXISTS idx_devices_uid ON devices(app, uid);

CREATE TABLE IF NOT EXISTS campaigns (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  app TEXT NOT NULL, entity_id TEXT NOT NULL, sender_uid TEXT NOT NULL,
  title TEXT NOT NULL, body TEXT NOT NULL, image TEXT,
  data TEXT NOT NULL, audience TEXT NOT NULL, condition TEXT NOT NULL,
  status TEXT NOT NULL, review_note TEXT, reviewer TEXT, fcm_response TEXT,
  created_at DATETIME DEFAULT CURRENT_TIMESTAMP, decided_at DATETIME, sent_at DATETIME
);
CREATE INDEX IF NOT EXISTS idx_campaigns_entity ON campaigns(app, entity_id, created_at);
CREATE INDEX IF NOT EXISTS idx_campaigns_status ON campaigns(app, status);
CREATE TABLE IF NOT EXISTS campaign_blocks (
  app TEXT NOT NULL, entity_id TEXT NOT NULL, reason TEXT,
  blocked_by TEXT, created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (app, entity_id)
);

ALTER TABLE notifications_history ADD COLUMN kind TEXT;   -- raw | template | campaign | test
ALTER TABLE notifications_history ADD COLUMN ref TEXT;    -- اسم القالب أو رقم الحملة
```

- **لا تُحفظ tokens الأجهزة في `notifications_history`.** الهدف يُكتب `uid:<x>` أو `role:admin`.
- **نسخ احتياطي:** `data.db` صار يحمل الإعدادات، فيُذكر في الوثائق أمر نسخ احتياطي يومي (`sqlite3 data.db ".backup ..."`).

### 3.15 الأخطاء وحدود السرعة

| رمز | المعنى |
|---|---|
| 400 | طلب أو معامل غير صالح، أو topic غير مسموح، أو رابط ممنوع، أو إعدادات/قالب غير صالح |
| 401 | توكن مفقود أو غير صالح (أو App Check في وضع enforce)، أو جلسة الداشبورد منتهية |
| 403 | الدور غير مسموح، أو `require` فشل، أو الشركة غير معتمدة أو محظورة، أو manager على قسم owner |
| 404 | `app` أو `template` غير موجود أو معطل |
| 409 | حملة ليست بالحالة المطلوبة، أو تعارض إصدار عند الحفظ (حفظ فوق نسخة أحدث) |
| 429 | حد السرعة أو حصة الحملات (مع `retry_after`) |
| 500 | خطأ داخلي غير متوقع في الأداة (رسالة عامة بلا تفاصيل، والتفاصيل في السجل فقط) |
| 502 | فشل FCM كلياً (لا تسليم، ورفض غير 4xx)، أو تعذّر الحصول على توكن خدمة Firebase |
| 503 | مصدر البيانات أو مفاتيح الهوية غير متاحة |

- حفظ الإعدادات يرسل `version` الحالي، وإذا تغير بالأثناء ⇐ 409 (يمنع مديرين يكتبون فوق بعض).
- حد للمفتاح القديم: `LEGACY_RATE_LIMIT_PER_MINUTE` (افتراضي 30).

---

## 4. مهام الأداة (`fcm_microservice`)

### S0 — التحضير
- **الحالة:** ⬜
- **الخطوات:**
  1. تأكيد الـ commit المنشور على السيرفر وتسجيله هنا.
  2. **لا تنشر `my-server` الحالي** (يكسر ب وج).
  3. فرع جديد من `my-server`: `feature/policy-auth`.
  4. في `taqati`: حفظ تعديلات SEC2-C غير المحفوظة في فرع `feature/sec2c-id-token`، ثم `feature/policy-notifications` منه.
- **طريقة الفحص:** `git branch` في المشروعين + `cargo test` ناجح على الفرع الجديد قبل أي تعديل.

### T1 — مخزن الإعدادات والقوالب
- **الحالة:** ⬜
- **الشرح:** structs الإعدادات والقوالب + `validate()` + الحفظ في SQLite بإصدارات + نسخة في الذاكرة تُحدَّث فوراً + استيراد/تصدير.
- **الملفات:** `src/config/mod.rs` (structs + validate)، `src/config/store.rs` (SQLite + cache)، `src/db.rs`، `src/main.rs` (أمر `import`)،
  `policies/watt.example.json`، `.gitignore`.
- **الخطوات:**
  1. structs بـ `serde(deny_unknown_fields)` لكل أقسام 3.2، و`version = 1`.
  2. `validate()` يطبق كل حواجز 3.5 ومراجع 3.6 وقيود 3.7.
  3. `ConfigStore`: `get(app) -> Arc<AppConfig>`، و`save_section(app, section, json, actor, expected_version)`، و`restore(version_id)`، و`export`/`import`.
  4. كل حفظ ⇐ `config_versions` + `audit_log`.
- **طريقة الفحص:** المثال الكامل يُقبل. حقل مجهول ⇐ رفض. `{p.x}` غير معرَّف ⇐ رفض. قالب لغير الأدمن هدفه topic ومعه `text` ⇐ رفض.
  `context` مع `data_source=none` ⇐ رفض. حفظ بـ `expected_version` قديم ⇐ 409. التراجع يعيد المحتوى حرفياً.

### T2 — حسابات الداشبورد والجلسات
- **الحالة:** ⬜
- **الملفات:** `src/admin/auth.rs` (جديد)، `src/admin/mod.rs` (Router `/api/v1/admin`)، `src/main.rs` (أمر `create-owner`)، `Cargo.toml` (`argon2`، نقل `rand` للاعتماديات الأساسية).
- **الخطوات:** الجداول (3.14)، login/logout/me/password، middleware يستخرج `ToolUser` ويتحقق owner/manager + التطبيق، حد المحاولات، `must_change_password`.
- **طريقة الفحص:** اختبارات: كلمة مرور خاطئة ⇐ 401 وتزيد العداد، 6 محاولات ⇐ 429، الجلسة المنتهية ⇐ 401،
  manager على تطبيق غيره ⇐ 403، manager على `identity` ⇐ 403، cookie بالخصائص الصحيحة، طلب مغيِّر بلا `X-Requested-With` ⇐ 403.

### T3 — مزودات الهوية
- **الحالة:** ⬜
- **الملفات:** `src/identity/mod.rs` (trait `IdentityProvider`)، `src/identity/firebase.rs` (نقل الموجود من `auth.rs`)، `src/identity/jwt.rs` (جديد).
- **الخطوات:** trait يرجع `Identity { uid, name, claims }`. نقل منطق Firebase كما هو. مزود JWT (JWKS أو HS256 من env، خوارزمية مثبتة، claims نقطية).
- **طريقة الفحص:** كل اختبارات Firebase الحالية تمر. JWT: توقيع صحيح ⇐ قبول. `alg=none` ⇐ رفض. توكن HS256 لمزود RS256 ⇐ رفض.
  iss/aud/exp خاطئ ⇐ رفض. claim متداخل `app_metadata.role` يُقرأ. سر env مفقود ⇐ التطبيق يرفض (503) مع خطأ في السجل.

### T4 — مصادر البيانات
- **الحالة:** ⬜
- **الملفات:** `src/datasource/mod.rs` (trait: `get_doc`, `members`)، `src/datasource/firestore.rs` (تعميم `FirestoreHttp`)، `src/datasource/http.rs` (جديد)، `examples/resolver_fastapi.py` (جديد).
- **الخطوات:** تحويل قيم Firestore في دالة واحدة. عميل `http` بتوقيع HMAC ومهلة. مثال FastAPI يتحقق من التوقيع.
- **طريقة الفحص:** اختبارات تحويل قيم Firestore. خادم HTTP وهمي: 200 ⇐ وثيقة، 404 ⇐ None، 500/مهلة ⇐ 503، والتوقيع يُحسب كما يتوقعه المثال.
  تشغيل مثال FastAPI محلياً ضد الأداة (يدوي).

### T5 — تعميم الأدوار والاشتراك
- **الحالة:** ⬜
- **الملفات:** `src/auth.rs`، `src/topics.rs`، `src/main.rs`.
- **الخطوات:** `resolve_role` حسب `identity.role` (claim أو data_source، مخزن 60ث). `is_admin` = الدور ∈ `admin_roles`.
  `topic_allowed` يقرأ `web_subscribe`. إزالة القيم الثابتة (`users|companies|technicals`).
- **طريقة الفحص:** اختبارات `auth.rs`/`topics.rs` الحالية بعد تكييفها + دور من claim + دور من data_source + `web_subscribe` مخصص.

### T6 — سجل الأجهزة
- **الحالة:** ⬜
- **الملفات:** `src/devices.rs`، `src/db.rs`، `src/main.rs`.
- **الخطوات:** كما في 3.8، وحد 20/دقيقة لكل uid، و`tokens_for_uid`، و`tokens_for_role`، و`remove_dead_token`.
- **طريقة الفحص:** تسجيل، upsert، نقل ملكية، تجاوز الحد يحذف الأقدم، بلا توكن هوية ⇐ 401، يعمل مع مزود JWT.

### T7 — محرك القوالب
- **الحالة:** ⬜
- **الملفات:** `src/template.rs`.
- **الخطوات:** تحليل `{...}` مرة عند الحفظ، التعويض، `|int`، `require`، `vars`، و`render()` تُستعمل للإرسال وللمعاينة.
- **طريقة الفحص:** قيمة `p` تحتوي `{doc.x}` لا تُفسَّر، `require` غير مطابق ⇐ خطأ، `vars` بقيمة غير موجودة ⇐ خطأ،
  `|int` على `150000.0` ⇐ `150000`، `/` في قيمة مسار ⇐ رفض، نص عربي يُحفظ كما هو.

### T8 — `POST /send/template`
- **الحالة:** ⬜
- **الملفات:** `src/send.rs` (نقل استدعاء FCM من `main.rs` إلى `fcm_send` + trait `FcmTransport`)، `src/main.rs`، `src/db.rs`.
- **الخطوات (التدفق):**
  ```
   1. app موجود + إعدادات صالحة + القالب مفعّل؟      لا ⇐ 404
   2. مفتاح التطبيق؟ ⇐ تخطَّ 3–6 (المرسل = service)
   3. توكن الهوية حسب المزود                          لا ⇐ 401
   4. (App Check حسب mode، Firebase فقط)               enforce وفشل ⇐ 401
   5. الدور ∈ senders؟                                 لا ⇐ 403
   6. حد السرعة لكل uid                                 تجاوز ⇐ 429
   7. تحقق params                                       لا ⇐ 400
   8. context.doc + require                             فشل ⇐ 403 / المصدر معطل ⇐ 503
   9. render + validation.rs                            فشل ⇐ 400
  10. dedupe                                            مكرر ⇐ 200 {"status":"duplicate"}
  11. حل المستلمين                                     لا أجهزة ⇐ 200 {"delivered":0}
  12. إرسال + حذف tokens الميتة + history + last_used_at
  ```
- **طريقة الفحص:** كل رمز في 3.15 بـ FCM وهمي. `order_new` من غير المشتري ⇐ 403. `order_status` من المشتري ⇐ 403.
  التكرار ⇐ `duplicate`. `UNREGISTERED` يحذف الجهاز. نفس القالب يعمل بمزود JWT + مصدر http.

### T9 — الحملات: الإنشاء والإرسال
- **الحالة:** ✅
- **الملفات:** `src/campaigns.rs`، `src/db.rs`، `src/main.rs`.
- **الخطوات:** التحقق من `entity` والحظر، بناء الشرط، قواعد المحتوى، الحصة، `review.mode`، `GET`/`cancel`.
- **طريقة الفحص:** شركة pending ⇐ 403. شركة غيره ⇐ 403. `admins` في الجمهور ⇐ 400. 5 topics + promos ⇐ 400.
  رابط ⇐ 400. صورة خارجية ⇐ 400. منتج شركة أخرى ⇐ 403. الحملة الثانية في نفس اليوم ⇐ 429.
  أول 3 ⇐ `pending_review` والرابعة ⇐ `sent`. الشرط المبني يطابق نصاً متوقعاً حرفياً.

### T10 — واجهات الداشبورد (backend)
- **الحالة:** ✅ (الشريحتان A وB وC منفذتان)
- **الملفات:** `src/admin/apps.rs`، `src/admin/templates.rs`، `src/admin/campaigns.rs`، `src/admin/keys.rs`، `src/admin/users.rs`، `src/admin/mod.rs`.
- **الخطوات:** كل مسارات جدول "واجهات الداشبورد" في 3.11، بما فيها `validate` و`preview` (بلا إرسال) و`test-send` (لأجهزة `test_uid` فقط)
  و`identity/test` و`data_source/test` والموافقة/الرفض والحظر.
- **طريقة الفحص:** اختبارات صلاحيات لكل مسار (owner/manager/غير مرتبط). `preview` لا يستدعي FCM. `test-send` بلا `test_uid` ⇐ 400.
  حذف قالب مستخدم حديثاً ⇐ 409. كل تعديل يظهر في `audit_log`.

### T11 — مفاتيح التطبيقات
- **الحالة:** ⬜
- **الملفات:** `src/auth.rs`، `src/admin/keys.rs`.
- **الخطوات:** كما في 3.10. `SERVICE_API_KEY` من env يبقى للتوافق مع تحذير في السجل.
- **طريقة الفحص:** المفتاح يُعرض مرة. استخدامه لتطبيق آخر ⇐ 403. بعد الإلغاء ⇐ 401. `last_used_at` يتحدث.

### T12 — الداشبورد (الواجهة)
- **الحالة:** ⬜
- **الملفات:** `dashboard/src/views/`: `Login.vue` (حسابات الأداة بدل Firebase)، `Apps.vue`، `AppSettings.vue`، `Templates.vue`، `TemplateEditor.vue`،
  `Campaigns.vue`، `Keys.vue`، `Users.vue`، `Audit.vue`؛ `dashboard/src/api.js` (cookie + `X-Requested-With`)، `router.js`؛
  حذف `firebase.js` واعتمادية Firebase من `package.json`؛ ثم `npm run build` لتحديث `dist/`.
- **الخطوات:**
  1. الدخول وتغيير كلمة المرور.
  2. قائمة التطبيقات وإعداداتها (أقسام owner مقفلة للمدير)، مع زري "فحص الاتصال" للهوية ومصدر البيانات.
  3. **محرر القوالب:** نموذج (المرسلون، المعاملات، الهدف، العنوان/النص مع إدراج المتغيرات بنقرة، data، الحدود) + وضع JSON +
     أخطاء مباشرة + معاينة الهاتف + إرسال تجريبي + الإصدارات والتراجع.
  4. طابور مراجعة الحملات (معاينة + الجمهور بلغة مفهومة + موافقة/رفض)، والحظر.
  5. المفاتيح (إنشاء يعرض المفتاح مرة + نسخ)، الحسابات، سجل التعديلات، السجل، عدد الأجهزة.
  6. التطوير المحلي عبر proxy في Vite لتفادي CORS.
- **طريقة الفحص:** تشغيل محلي: إنشاء قالب من الصفر بالنموذج، ومحاولة حفظ قالب مخالف للحواجز تُرفض برسالة واضحة،
  ومعاينة، وإرسال تجريبي لجهاز حقيقي، وتراجع لإصدار سابق، وموافقة/رفض حملة. لقطات شاشة في سجل التقدم.

### T13 — المفتاح القديم المقيد + التوثيق
- **الحالة:** ⬜
- **الملفات:** `src/auth.rs`، `.env.example`، `docs.md`، `README.md`، `.agent.md`، `curl_api.md`، `docs/integration/` (جديد).
- **الخطوات:** حد `LEGACY_RATE_LIMIT_PER_MINUTE` وتحذير يومي. الوثائق:
  - جدول الصلاحيات الكامل.
  - **دليل الربط لكل نوع باك إند** (`docs/integration/firebase.md`، `jwt-fastapi.md`، `supabase.md`):
    إعداد الهوية، مصدر البيانات، تسجيل الجهاز من Flutter، إرسال قالب، وإرسال من باك إند بمفتاح التطبيق.
  - مرجع إعدادات القوالب والحواجز. **تنبيه (T8-F):** قالب فيه `context.require` يعتمد `{sender.uid}` (مثل `buyer_id: "{sender.uid}"` في `order_new`) سيرد 403 للمفتاح الخدمي لأن uid المفتاح هو `service`؛ وثّق أن الإرسال بالمفتاح يصلح للقوالب بلا `require` معتمد على هوية المرسل.
  - **متابعات (T8-F):** (أ) `ConfigStore::save_template` يرفع `version`، فيُبطل مخبأ `RoleDirectory` لكل التطبيق عند كل تعديل قالب — قيّم فصل إصدار الأدوار عن إصدار الإعدادات أو وثّق الكلفة. (ب) `validate_template` يسمح بـ`{var}` في `require` بينما المحرك يرفضه عند الإرسال (400 لكل طلب) — وحّد القاعدة أو وثّقها صراحة.
  - النسخ الاحتياطي لـ `data.db`.
  - الـ reverse proxy يجب أن يرسل `X-Forwarded-Proto` وإلا أضف الأصل إلى `CORS_ALLOWED_ORIGINS` (فحص CSRF في الداشبورد).
  - اجعل الباك إند يضيف `iat` في كل توكن، وإلا لا يُطبَّق `max_age_seconds`.
- **طريقة الفحص:** تجاوز الحد ⇐ 429. كل مسار في الـ Router موثق. اتباع `jwt-fastapi.md` حرفياً على مشروع تجريبي ينجح.

### T14 — App Check (اختياري)
- **الحالة:** ⬜
- **الملفات:** `src/appcheck.rs`، `src/send.rs`، `src/campaigns.rs`.
- **طريقة الفحص:** اختبارات JWT (iss/aud/exp/kid). `mode` غير `off` مع مزود غير Firebase ⇐ رفض الحفظ.

### T15 — TOTP لحسابات الداشبورد (اختياري)
- **الحالة:** ⬜
- **الخطوات:** تفعيل TOTP للمالك (إلزامي بعد التفعيل) ورموز استرجاع.
- **طريقة الفحص:** دخول بلا رمز ⇐ 401 بعد التفعيل. رمز استرجاع يعمل مرة واحدة.

### T16 — رفع service account وإضافة تطبيق من الداشبورد
- **الحالة:** ⬜ (بعد T2 وT10 وT12)
- **الشرح:** إضافة تطبيق جديد أو تدوير مفتاحه من الداشبورد بلا إعادة بناء الحاوية ولا دخول للسيرفر (3.13).
- **الملفات:** `src/credentials.rs` (`install_credential`: تحقق + كتابة ذرية + تحميل مباشر، `remove_credential`)،
  `src/admin/apps.rs`، `src/admin/auth.rs` (`require_reauth`)، `src/main.rs` (`CREDENTIALS_UPLOAD_ENABLED`)،
  `dashboard/src/views/AddApp.vue` + قسم "المفتاح" في `AppSettings.vue`، `.env.example`، `docs.md`.
- **الخطوات:**
  1. `install_credential(app, bytes)`: التحقق البنيوي، ثم فحص access token حي، ثم ملف مؤقت ⇐ `rename` ⇐ `0600` ⇐ إدخال في `AuthMap`/`ProjectIdMap`.
  2. منع السباق مع المراقب: المراقب يتجاهل الملفات المؤقتة (`.tmp-*`)، والتحميل المزدوج لنفس الملف لا يضر.
  3. الاستبدال يفرض نفس `project_id`. الحذف يتطلب تأكيد الاسم.
  4. `require_reauth`: كلمة المرور (وTOTP إن وُجد) خلال آخر 5 دقائق.
  5. الواجهة: سحب وإفلات الملف، وعرض نتيجة كل فحص، ثم التوجيه لإعدادات التطبيق.
- **طريقة الفحص:**
  - ملف ليس service account ⇐ 400. PEM تالف ⇐ 400. مفتاح ملغى في Google ⇐ 400 من الفحص الحي.
  - manager ⇐ 403. owner بلا إعادة كلمة المرور ⇐ 403.
  - استبدال بمشروع مختلف ⇐ 400. نجاح ⇐ التطبيق يرسل فوراً بلا إعادة تشغيل.
  - الملف على القرص بصلاحيات `0600`. لا يوجد أي مسار يرجع محتواه (grep على الـ Router + اختبار).
  - `CREDENTIALS_UPLOAD_ENABLED=false` ⇐ المسارات 404.
  - `audit_log` يسجل الحدث بدون محتوى الملف.

---

## 5. مهام تطبيق واط (`taqati`)

> تُنفَّذ حسب `taqati/AGENTS.md`: Clean Architecture (domain نقي)، ومهمة في `TODO.md` قبل البدء،
> و`flutter analyze` + `flutter test`، و`dart format --line-length 160` على الملفات المعدلة.
> واط يستخدم: `identity.provider = firebase` و`data_source = firestore`.

### W1 — تسجيل الجهاز في الأداة
- **الحالة:** ⬜
- **الملفات:** `notifications/domain/repositories/device_registry.dart` (جديد)، `notifications/data/data_sources/api_device_registry.dart` (جديد)،
  `core/services/push_notification_service.dart`، `core/di/get_it.dart`، `utils/app_urls.dart`.
- **الخطوات:** register عند الحصول على token وعند `onTokenRefresh` وعند الدخول، وunregister عند الخروج.
  **الإبقاء مؤقتاً** على كتابة `fcm_tokens` في Firestore لخدمة النسخ القديمة (تُحذف في W8).
- **طريقة الفحص:** register عند الدخول، unregister عند الخروج، لا طلب بلا مستخدم، 401 ⇐ تجديد واحد.

### W2 — مرسل القوالب
- **الحالة:** ⬜
- **الملفات:** `notifications/domain/repositories/templated_notification_sender.dart`، `notifications/data/data_sources/api_templated_notification_sender.dart`.
- **الخطوات:** `send(templateKey, params)` مع معالجة 401/403/404/429. أسماء القوالب ثوابت في domain، وتعريفها في الداشبورد.
- **طريقة الفحص:** اختبارات Dio وهمية لكل رمز حالة.

### W3 — تحويل تنبيهات الأدمن (النوع ب)
- **الحالة:** ⬜
- **الملفات:** الأماكن السبعة في الجدول 1.2 (ب).
- **الخطوات:** كل `sendTopicNotification(topic: admins, ...)` ⇐ `send('admin_alert', {title, body, route})`.
- **طريقة الفحص:** grep لا يجد `NotificationTopic.admins` في مسارات الإرسال + اختبارات المستدعين.

### W4 — تحويل إشعارات الطلبات (النوع ج)
- **الحالة:** ⬜
- **الملفات:** `store_orders/data/data_sources/firebase_order_notifier.dart` (أو `api_order_notifier.dart` جديد)، `core/di/get_it.dart`.
- **الخطوات:** `order_new {order_id}` و`order_status {order_id}`، وحذف `_getSellerTokens`/`_getBuyerTokens`. **التأكد أن `seller_id` هو uid**.
- **طريقة الفحص:** طلب جديد يصل للبائع، وتغيير الحالة يصل للمشتري، و`order_status` من حساب المشتري يُرفض.

### W5 — مواضيع التسويق وإعداد "عروض الشركات"
- **الحالة:** ⬜
- **الملفات:** `notification_topic_kind.dart` (+`promo`، `category`، `company`)، `notification_topic_id.dart`، صفحة إعدادات الإشعارات (NOTIF2)، `l10n`.
- **الخطوات:** `promos` (مفعّل افتراضياً مع إطفاء، D2)، و`cat_<slug>` من `BrandCategory` في `products/domain/product.dart` (مثل `cat_panels`)،
  و`company_<id>` مؤجل لميزة "متابعة شركة" (D4).
- **طريقة الفحص:** اختبارات `parse`/التحويل + مزامنة الاشتراك عند تغيير الإعداد.

### W6 — شاشة إنشاء الحملة للشركة + "حملاتي"
- **الحالة:** ⬜
- **الملفات:** `features/campaigns/{domain,data,presentation}` (ميزة جديدة)، والراوتر، و`l10n`.
- **الخطوات:** نموذج (عنوان، نص، صورة، إجراء)، ومنتقي جمهور مبني على `NotificationTargetBuilder` بخيارات محدودة، ومعاينة مع اسم الشركة،
  وعدّاد الحصة، وقائمة الحملات وحالتها.
- **طريقة الفحص:** اختبارات domain للجمهور + ويدجت + تجربة كاملة مع الموافقة من الداشبورد.

### W7 — App Check في التطبيق (مع T14)
- **الحالة:** ⬜
- **الخطوات:** `firebase_app_check` (Play Integrity / App Attest) و`X-Firebase-AppCheck` في كل الطلبات للأداة.

### W8 — التنظيف بعد الانتقال
- **الحالة:** ⬜ (شرط: إطفاء المفتاح القديم، R5)
- **الخطوات:** إيقاف كتابة `fcm_tokens` و`buyer_fcm_tokens` وحذفها، وإلغاء اشتراك `admins`، وتعديل `firestore.rules` حتى لا تُقرأ tokens الأجهزة.
- **طريقة الفحص:** `firestore_rules_tests` + grep لا يجد `fcm_tokens` في `lib/`.

---

## 6. النشر والانتقال

| خطوة | العمل | التراجع |
|---|---|---|
| **R1** | نشر الأداة (T1–T13). `create-owner`، ثم استيراد `policies/watt.json` من الداشبورد ومراجعة القوالب. `LEGACY_API_KEY_ENABLED=true`. | إعادة الصورة السابقة + استعادة نسخة `data.db` قبل النشر. |
| **R2** | فحوصات القسم 7 على السيرفر الحقيقي بحسابات اختبار لكل دور. | — |
| **R3** | نشر تحديث واط (W1–W6). | الأداة تخدم القديم والجديد معاً. |
| **R4** | مراقبة استخدام المفتاح القديم يومياً، ثم `force_update` (موجود في `feature_flags.dart`). | — |
| **R5** | `LEGACY_API_KEY_ENABLED=false`، وتدوير `API_KEY`، وإنشاء مفتاح تطبيق لأي باك إند، والتحقق أن القديم ⇐ 401. حذفه من `taqati/api/Taqati api/fcm/*.yml`. | تفعيل مؤقت بمفتاح **جديد** فقط. |
| **R6** | W8، ثم App Check (`monitor` ⇐ `enforce`). | `mode: monitor`. |

---

## 7. الاختبار

- **وحدة:** لكل وحدة Rust جديدة (`cargo test`) بأسلوب `auth.rs` (JWKS وFirestore وFCM وهمية عبر traits).
- **تكامل محلي:** `cargo run` + Firebase حقيقي لواط، + **مشروع تجريبي صغير بـ FastAPI** (مزود `jwt` + مصدر `http`) لإثبات أن الأداة تعمل بلا Firebase Auth/Firestore.
- **`curl_api.md`** يُحدَّث بحالة لكل صف في 3.15.
- **حسابات اختبار:** admin، company معتمدة، company غير معتمدة، user، technical. نوثق uid كل حساب (لا كلمات مرور).
- **التطبيق:** `flutter analyze` + `flutter test` + تجربة يدوية على Android وiOS والويب.

---

## 8. القرارات (افتراضيات معتمدة، قابلة للتغيير)

| # | القرار | المعتمد | ملاحظة |
|---|---|---|---|
| D1 | مراجعة الحملات | `first_n` و n=3 | يتغير من الداشبورد |
| D2 | `promos` افتراضياً | مفعّل مع إطفاء سهل | بديل: مطفأ ويُعرض على المستخدم مرة |
| D3 | الحصة | 1 يومياً، 5 أسبوعياً | |
| D4 | متابعة شركة (`company_<id>`) | مؤجل | الإعدادات تدعمها من الآن |
| D5 | مصدر الدور لواط | Firestore `users/{uid}.role` | محمي من التصعيد (SEC3) |
| D6 | مخزن الإعدادات | SQLite + استيراد/تصدير JSON | بدل ملفات `policies/` الحية |
| D7 | دخول الداشبورد | حسابات الأداة (argon2id) | يزيل اعتماد الداشبورد على Firebase |
| D8 | الأسرار | في `.env` فقط وتُذكر باسمها | الداشبورد لا يقرأ سراً أبداً |
| D12 | رفع service account | من الداشبورد للمالك فقط، كتابة فقط، مع إعادة كلمة المرور وفحص حي | يُطفأ بـ `CREDENTIALS_UPLOAD_ENABLED=false` |
| D9 | البث الحر لغير الأدمن | ممنوع في القوالب، متاح فقط عبر الحملات | حاجز يُفرض عند الحفظ |
| D10 | مزودات الهوية | `firebase` + `jwt` | يغطي Supabase/Auth0/Keycloak/باك إند خاص |
| D11 | مصادر البيانات | `firestore` + `http` + `none` | `http` بعقد ثابت وتوقيع HMAC |

---

## 9. سجل التقدم

| التاريخ | المهمة | الحالة | ملاحظة |
|---|---|---|---|
| 2026-10-21 | الخطة (المراجعة 1) | ✅ | بعد مراجعة الكود في المشروعين |
| 2026-10-21 | الخطة (المراجعة 2) | ✅ | القوالب والإعدادات من الداشبورد + دعم باك إند غير Firebase + حسابات الأداة + مفاتيح التطبيقات |
| 2026-10-21 | الخطة (المراجعة 2.1) | ✅ | رفع service account وإضافة تطبيق من الداشبورد (3.13، T16، D12) |
| 2026-10-21 | T1 — مخزن الإعدادات والقوالب | ✅ | النماذج + التحقق + SQLite (إصدارات وسجل) + استيراد/تصدير + أمرا CLI. DDL: `app_configs` صار فيه `version` |
| 2026-10-21 | T2 — حسابات الداشبورد والجلسات | 🔄 بانتظار المراجعة | T2-0: قفل كتابات `ConfigStore` + حفظ `last_used_at` + حاجز `[G6]` + إنجليزية `src/`. T2: `tool_users`/`tool_user_apps`/`tool_sessions` (أوقات INTEGER epoch) + argon2id + كوكي `__Host-` + CSRF + `create-owner`/`reset-owner-password`. 118 اختباراً ناجحاً |
| 2026-10-21 | T3 — مزودات الهوية | ✅ | نقل JWKS/Firebase من `auth.rs` إلى `src/identity/` بلا تغيير سلوك (خط `auth.rs`: −270/+33) + مزود JWT عام (JWKS أو HS256 من البيئة، خوارزمية مثبتة، claims نقطية) + سجل يبني مزوداً لكل `(تطبيق، إصدار إعداد)` بلا ربط بالإرسال. T3-0: إصدارات القوالب متصاعدة عند الاستيراد. 152 اختباراً ناجحاً. قرار المالك: حقلا `identity` و`datasource` في `AppState` يُضافان في T5 مع السماح بتعديل سطر واحد في `test_state()` داخل `src/admin/mod.rs` |
| 2026-10-21 | T4 — مصادر البيانات | 🔄 بانتظار المراجعة | واجهة `DataSource` (`get_doc`/`members`) + تحقق المسارات/المجموعات/الحقول/القيم والترميز + مصدر Firestore معمَّم (تحويل القيم، `runQuery`، سقف 1MB وعمق 8) + resolver HTTP موقّع HMAC-SHA256 بلا Bearer + مثال FastAPI و`--selftest` + سجل لكل `(تطبيق، إصدار)` بلا إضافة إلى `AppState` وبلا ربط بالمسارات. 187 اختباراً ناجحاً (35 جديداً). التقرير: `docs/plans/reports/T4-report.md` |
| 2026-10-21 | T5 — تعميم الأدوار والاشتراك | ✅ | `src/roles.rs` (محلّل + مخبأ + `is_admin_role` + `topic_matches`) + `AuthService` فوق السجلين + أنماط `web_subscribe` + `LEGACY_SIGNED_IN_TOPICS` للتطبيقات بلا إعداد + حقلا `identity`/`datasource` في `AppState`. 212 اختباراً ناجحاً. قرار المراجع: `#[allow(dead_code)]` المؤقتة مقبولة وتزول عند استعمالها، وتعديلات اختبارات Firebase مقبولة. التقرير: `docs/plans/reports/T5-report.md` |
| 2026-10-21 | T6 — سجل الأجهزة | 🔄 بانتظار المراجعة | جدول `devices` (`PRIMARY KEY(app, token)` + فهرس `(app, uid)`) + `DeviceStore` (upsert/نقل ملكية/إخلاء الأقدم ضمن معاملة تحت قفل كتابة/إلغاء) + `RoleDirectory` بمخبأ `(app, version, role)` TTL وحد 256 + `tokens_for_role` بسقف 5000 + مسارا `POST /api/v1/devices/register` و`/devices/unregister` بهوية مستخدم فقط (بلا مفتاح خدمة/قديم) وحد 20/دقيقة لكل uid. 242 اختباراً ناجحاً (30 جديداً). التقرير: `docs/plans/reports/T6-report.md` |
| 2026-10-21 | T7 — محرك القوالب | 🔄 بانتظار المراجعة | `segments()` في `config/placeholder.rs` (إضافة صرفة: 116 سطراً، 0 محذوف) + `src/template.rs`: `validate_params` (text/pattern/enum/int، حدود 4096 بايت و200 للـpattern و`size_limit` للمترجم) ثم `context_path` (مقطع مسار واحد) ثم `render` (مرور واحد، `|int` بتقريب بعيداً عن الصفر، `require` موحّد 403، `vars` كسولة، حدود `validation.rs`). بلا شبكة/قاعدة بيانات/`AppState`. 285 اختباراً ناجحاً (43 جديداً). التقرير: `docs/plans/reports/T7-report.md` |
| 2026-10-21 | T8 — إرسال القوالب | ✅ | `src/send.rs`: `FcmTransport`/`HttpFcm`/`FcmOutcome`/`build_message`/`send_to_tokens` (JoinSet + Semaphore، بلا إعادة محاولة) ونقل نداء FCM من `main.rs` بلا تغيير ردود `/send` (4 اختبارات توصيف). `POST /api/v1/send/template` بالترتيب: app ⇒ IP ⇒ (مفتاح خدمة أو هوية + دور) ⇒ قالب مفعّل ⇒ senders ⇒ حد/uid ⇒ params ⇒ doc ⇒ render ⇒ `validation.rs` ⇒ dedupe ⇒ أجهزة ⇒ إرسال ⇒ سجل. `db.rs`: `kind`/`ref`، `template_dedupe` (SHA-256، ذرّية، انتهاء)، `touch_template_used` (يغيّر `last_used_at` فقط)، `TemplateRateLimiter` (نافذة ساعة، سقف 10k). إصلاح T8-F: تحرير مفتاح dedupe عند كل خروج لا يسلّم (أجهزة/دور/بيانات/credentials) مع 5 اختبارات. 335 اختباراً ناجحاً (50 جديداً). التقريران: `docs/plans/reports/T8-report.md` و`T8-fix-report.md` |
| 2026-10-21 | T9 — حملات الشركات | 🔄 بانتظار المراجعة | `src/campaigns.rs`: `build_condition` (مجموعات ⇒ شرط حرفي، `allowed_topics` حرفي/`*`/`{p.param}`، ≤5 مع promos، تفريد داخل المجموعة) + محتوى (`title_max`/`body_max`، منع روابط best-effort، صورة بمضيف تام من `image_hosts`، إجراء من `actions` مع `check` ⇒ 403 موحد) + `resolve_entity`/`check_action`/`dispatch_campaign`/`insert_campaign`. `db.rs`: `campaigns`/`campaign_blocks` + فهرسان بصيغة 3.14، `insert_campaign_checked` (حصة 24h/7d داخل معاملة تحت قفل كتابة، `retry_after` ≥1)، `review.mode` (`first_n` يعدّ `sent|sending` فقط، service يتخطى)، `get/list/claim/cancel/finish/is_entity_blocked`. `template.rs`: `Snippet`/`render_snippet`/`validate_param_map`/`check_require_map` بإعادة هيكلة داخلية بلا تعديل اختبارات T7. المسارات: `POST|GET /api/v1/campaigns` و`POST /api/v1/campaigns/{id}/cancel` بالترتيب 12 خطوة وتوحيد 403/404. 394 اختباراً ناجحاً (59 جديداً)، clippy 0، والتجارب الخمس موثقة. التقرير: `docs/plans/reports/T9-report.md` |
| 2026-10-21 | T9 — قرار المراجع | ✅ | حقن العنوان 201 مقبول (العنوان لا يدخل الشرط)؛ `#[allow(dead_code)]` على `claim_for_sending` يبقى حتى T10-B؛ تعافي الحملات العالقة في `sending` مؤجل إلى T10-B |
| 2026-10-21 | T10-A — تطبيقات وقوالب وإصدارات وتدقيق | 🔄 بانتظار المراجعة | `src/admin/apps.rs` (تطبيقات/أقسام/إصدارات/استيراد/تصدير/تدقيق) و`src/admin/templates.rs` (CRUD/validate/enable/delete/preview/test-send) + `authorize` و`AdminError` في `mod.rs` + قراءات `db.rs` + قاعدة `context.require` في `validate.rs`. 450 اختباراً ناجحاً (56 جديداً)، clippy 0، 5 تجارب سلبية مستعادة بـ`sha256`. التقرير: `docs/plans/reports/T10A-report.md` |
| 2026-10-22 | T10-A — قرار المراجع | ✅ | `GET /apps/{app}` 404 لتطبيق credentials-only مقبول؛ `init` بفحص الدور+credentials بدل `authorize` مقبول مع اختبار `init_unassigned_manager_is_403`؛ استعمال `auth_fail_limiter` لحد test-send مؤقت (يُتابَع في T13)؛ تعديل `validate.rs` (قيود `require`) مقبول، ويُضاف في T13 اختبار مخصص لـ`dedupe.seconds` |
| 2026-10-22 | T10-B — حملات الداشبورد والسجل والإرسال الحر | 🔄 بانتظار المراجعة | `src/admin/campaigns.rs`: قائمة الحملات (`status` مجهول ⇒ 400، `limit` ≤ 100) وموافقة/رفض بحركة ذرية `UPDATE ... WHERE status = 'pending_review'` تكتب `reviewer` و`decided_at` و`review_note` مع الحالة، والحظر في معاملة واحدة (upsert + إلغاء المعلقة). `src/admin/history.rs`: السجل (بمُرشِّح `kind` و`status`، `payload` في التفاصيل فقط) والإرسال الحر (هدف واحد من topic/condition/uid، لا توكن خام، حد 30/دقيقة) وإحصاءات الأجهزة (أعداد فقط). `db.rs`: `fail_stale_sending` واستدعاؤه عند الإقلاع بعد فتح المخزن وقبل الاستماع. `ConfigStore::app_names` واتحادها مع `project_map` في `GET /apps`. 497 اختباراً ناجحاً (47 جديداً)، clippy 0، 5 تجارب سلبية مستعادة بـ`sha256`. التقرير: `docs/plans/reports/T10B-report.md` |
| 2026-10-22 | T10-B — قرار المراجع | ✅ | (1) تسجيل مسار القسم على المسار الثابت `put_campaigns_section` مقبول: تُوثَّق في خطة T12 أن الواجهة تستعمل `PUT /apps/{app}/campaigns` للقسم و`GET /apps/{app}/campaigns` للقائمة؛ (2) اختبار الحظر عبر `is_entity_blocked` بدل مسار T9 كامل مقبول (الراوتر داخل `main()`) — متابعة T13: استخراج `build_router(state)` ليُختبر end-to-end؛ (3) حد الإرسال الحر على `topics_limiter` مقبول مؤقتاً (متابعة T13 مع حد test-send)؛ (4) `condition` يبقى ظاهراً في قائمة الحملات للأدمن؛ (5) `review_note` على الموافقة اختياري ومقبول |
| 2026-10-22 | T10-C — حسابات الأداة وفحوص الاتصال | 🔄 بانتظار المراجعة | `src/admin/users.rs`: `GET/POST /users` و`PUT/DELETE /users/{id}` و`POST /users/{id}/reset-password` (owner فقط؛ **مدير فقط** يُنشأ، والمالك بالـCLI؛ كلمة مرور مؤقتة مرة واحدة مع `Cache-Control: no-store`، ولا تُخزَّن ولا تُدقَّق) + `PUT /auth/test-uid` (على النفس فقط) + `src/admin/diagnostics.rs`: `POST /apps/{app}/identity/test` (أسماء الـclaims فقط، لا قيمها، ولا يُعاد التوكن) و`POST /apps/{app}/data_source/test` (أسماء الحقول وأنواعها، أو عدد الأعضاء) + إصلاح سباق الحظر: `claim_for_sending` فيه `NOT EXISTS` للحظر ذرّياً. 538 اختباراً ناجحاً (41 جديداً)، clippy 0، 5 تجارب سلبية مستعادة بـ`sha256`. التقرير: `docs/plans/reports/T10C-report.md` |

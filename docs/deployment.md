# دليل النشر والتشغيل (Deployment Guide)

يوثّق هذا الدليل متطلبات تشغيل خادم FCM المصغّر في بيئة الإنتاج، وضبط متغيرات البيئة،
وإعدادات الحاوية (Docker)، وتكوين خادم الويب العكسي (Reverse Proxy)، وإجراءات النسخ الاحتياطي،
وخطوات التحديث والتراجع الآمن.

---

## 1. جدول متغيرات البيئة الكامل

تُقرأ متغيرات البيئة عند بدء تشغيل الخادم من ملف `.env` أو من بيئة التشغيل:

| المتغير | القيمة الافتراضية | هل هو سر؟ | الوصف والأثر |
|---|---|---|---|
| `CORS_ALLOWED_ORIGINS` | *إلزامي (لا افتراضي)* | لا | قائمة أصول المتصفح المصرح لها مفصولة بفواصل (مثل `https://admin.example.com`). يمنع الخادم البدء إذا كان فارغاً أو احتوى على `*`. |
| `PORT` | `8080` | لا | منفذ TCP الذي يستمع عليه الخادم الداخلي. |
| `TRUST_PROXY` | `false` | لا | تفعيله (`true`) عند العمل خلف Reverse Proxy موثوق لاعتماد عنوان IP الحقيقي من ترويسة البروكسي. |
| `CLIENT_IP_HEADER` | *فارغ* | لا | اختياري. اسم ترويسة عنوان العميل التي يضبطها البروكسي الموثوق: `cf-connecting-ip` أو `x-real-ip` أو `x-forwarded-for` (حروف صغيرة). قيمة غير معروفة تمنع الإقلاع. عند تركه فارغاً يُعتمد **آخر** عنصر من `X-Forwarded-For` وليس أوله. |
| `DATABASE_URL` | `sqlite://data.db` | لا | مسار قاعدة بيانات SQLite ومحرك التخزين. |
| `CREDENTIALS_DIR` | `credentials` | لا | مجلد حفظ ملفات حساب الخدمة Google Service Account. يجب أن يكون قابلاً للكتابة للحاوية. |
| `CREDENTIALS_UPLOAD_ENABLED` | `true` | لا | السماح برفع واستبدال ملفات الاعتماد من لوحة التحكم للمالك. عند جعله `false` تُرد مسارات الرفع بـ `404`. |
| `COOKIE_SECURE` | `true` | لا | إرسال الكوكي بخاصية `Secure` وبادئة `__Host-fcm_session`. يُعطّل (`false`) فقط أثناء التطوير المحلي عبر HTTP بدون SSL. |
| `ADMIN_SESSION_HOURS` | `12` | لا | عمر جلسة لوحة التحكم بالساعات (مسموح بين 1 و 72 ساعة). |
| `SERVICE_API_KEY` | *اختياري (فارغ)* | **نعم** | مفتاح خادم عام للعمليات الآلية يمنح صلاحيات إرسال عامة عبر ترويسة `Authorization: Bearer`. |
| `LEGACY_API_KEY_ENABLED` | `false` | لا | تفعيل مسار المفتاح القديم التوافقي على `/api/v1/send` فقط. |
| `API_KEY` | *فارغ* | **نعم** | قيمة المفتاح القديم المتوافق مع الإصدارات السابقة أثناء فترة الانتقال. |
| `LEGACY_RATE_LIMIT_PER_MINUTE` | `60` | لا | سقف عدد طلبات المفتاح القديم في الدقيقة لجميع المتصلين معاً (يُرد بـ `429` عند التجاوز). |
| *متغيرات أسرار التطبيقات الديناميكية* | *حسب التكوين* | **نعم** | أسماء المتغيرات المحددة في `hs256_secret_env` لمزودات JWT أو `secret_env` لمصادر HTTP (مثل `CLIENT_JWT_SECRET` و`CLIENT_RESOLVER_SECRET`). لا تقل عن 32 بايت. |

---

## 2. النشر عبر Docker وDocker Compose

> **النشر العملي على السيرفر:** استعمل `deploy/deploy.sh` (نسخ احتياطي، تحديث، بناء، فحوصات، وتراجع) و`deploy/Caddyfile.example`.
> ملف `docker-compose.yml` في المستودع يربط **مجلد** `./data` (لا ملف `data.db` وحده) لأن SQLite بنمط WAL يحتفظ بملفي
> `-wal` و`-shm` بجانب القاعدة؛ ربط الملف المفرد يُضيّع آخر الكتابات عند إعادة إنشاء الحاوية. المنفذ مربوط على `127.0.0.1:8084`
> افتراضياً ليصله Caddy فقط (`BIND_ADDR` في `.env` لتغييره). المثال أدناه توضيحي عام.

يحتاج الخادم إلى وسيطي تخزين دائمين (Volumes):
1. مسار قاعدة البيانات `data.db` لضمان بقاء الإعدادات والحسابات وسجل الأجهزة.
2. مجلد الاعتماد `credentials/` ويجب أن يمتلك صلاحيات الكتابة ليتمكن المالك من رفع ملفات التطبيقات الجديدة من الواجهة.

### مثال `docker-compose.yml`:
```yaml
version: '3.8'

services:
  fcm_microservice:
    image: ghcr.io/example/fcm_microservice:v1.0.0
    restart: unless-stopped
    ports:
      - "127.0.0.1:8080:8080"
    environment:
      - PORT=8080
      - CORS_ALLOWED_ORIGINS=https://fcm-admin.example.com
      - TRUST_PROXY=true
      - COOKIE_SECURE=true
      - ADMIN_SESSION_HOURS=12
      - CREDENTIALS_UPLOAD_ENABLED=true
      - DATABASE_URL=sqlite:///data/data.db
      - CREDENTIALS_DIR=/data/credentials
      - SERVICE_API_KEY=${SERVICE_API_KEY}
      - LEGACY_API_KEY_ENABLED=false
    volumes:
      - ./data:/data
```

---

## 3. أول تشغيل: إنشاء المالك وتسجيل الدخول

عند تشغيل الخادم لأول مرة تكون قاعدة البيانات خالية من أي حساب إداري.
يُنشأ حساب المالك الأول حصراً عبر سطر الأوامر (CLI):

```bash
# إنشاء حساب المالك وطباعة كلمة مرور مؤقتة لمرة واحدة
docker compose exec fcm_microservice fcm_microservice create-owner --email admin@example.com
```

المخرجات:
```text
Owner account created for: admin@example.com
Temporary password: <GENERATED_TEMP_PASSWORD>
You must change this password on first login.
```

1. افتح رابط لوحة التحكم في المتصفح وسجّل الدخول بالبريد وكلمة المرور المؤقتة.
2. سيفرض النظام عليك فوراً تغيير كلمة المرور عبر شاشة إجبارية قبل السماح بالوصول لأي شاشة أخرى.

---

## 4. إعداد الخادم العكسي (Reverse Proxy) والأمان

يجب تشغيل الأداة خلف خادم عكسي يوفر تشفير HTTPS للأسباب التالية:
1. كوكي جلسة لوحة التحكم تحمل بادئة `__Host-` وتتطلب اتصال HTTPS آمن.
2. حماية ترويسات المصادقة وتوكنات الهوية من التنصت.

### 4.1 الترويسات الإلزامية وخاصية `TRUST_PROXY`
عند تفعيل `TRUST_PROXY=true` يعتمد الخادم عنوان IP العميل لمحددات السرعة وحماية محاولات الدخول الفاشلة.
القاعدة الفعلية في `src/auth.rs::client_ip`:
- `TRUST_PROXY=false` (الافتراضي): عنوان اتصال TCP (peer) دائماً، وتُتجاهل كل الترويسات.
- `TRUST_PROXY=true` مع `CLIENT_IP_HEADER` محدد: تُقرأ **هذه الترويسة فقط**، ويجب أن تحمل عنوان IP واحداً صالحاً.
  إن كانت مفقودة أو غير صالحة يُستخدم عنوان الـ peer، **ولا** يُعتمد `X-Forwarded-For` بديلاً.
- `TRUST_PROXY=true` بدون `CLIENT_IP_HEADER`: يُعتمد **آخر** عنصر في `X-Forwarded-For` (الذي أضافه البروكسي الموثوق)،
  وليس العنصر الأول الذي قد يزوّره العميل. السلسلة الأطول من 1024 بايت تُرفض ويُستخدم عنوان الـ peer.

**Cloudflare Tunnel (الإعداد الموصى به):** Cloudflare تضبط `CF-Connecting-IP` بقيمة العنوان الحقيقي،
ولا يستطيع العميل تزويرها عبر النفق. استخدم:
```env
TRUST_PROXY=true
CLIENT_IP_HEADER=cf-connecting-ip
```
**Nginx:** اجعل Nginx يضبط `X-Real-IP` بنفسه (`proxy_set_header X-Real-IP $remote_addr;`) واستخدم
`CLIENT_IP_HEADER=x-real-ip`، أو اترك `CLIENT_IP_HEADER` فارغاً مع `X-Forwarded-For` الذي يضيفه Nginx في نهاية السلسلة.

لذلك يجب على الخادم العكسي تمرير:
- `Host`
- `X-Real-IP`
- `X-Forwarded-For`
- `X-Forwarded-Proto https`

> [!WARNING]
> لا تضبط `CLIENT_IP_HEADER=x-forwarded-for` إلا إذا كان البروكسي **يستبدل** الترويسة بعنوان واحد.
> الترويسة التي تحمل قائمة عناوين تُعد غير صالحة في هذا الوضع، ويُستخدم عنوان الـ peer.

### 4.2 الحماية من هجمات CSRF
تتحقق الأداة من تطابق ترويسة `Origin` في جميع طلبات تعديل الحالة (`POST`, `PUT`, `DELETE`)
مع قائمة `CORS_ALLOWED_ORIGINS`. أي طلب يحمل `Origin` غريباً يُرد فوراً بـ **`403 Forbidden`**.

### 4.3 مثال تكوين Nginx:
```nginx
server {
    listen 443 ssl http2;
    server_name fcm-admin.example.com;

    ssl_certificate /etc/letsencrypt/live/fcm-admin.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/fcm-admin.example.com/privkey.pem;

    client_max_body_size 16k; # يمنع الحمولات الضخمة؛ رفع الاعتماد مقيد بـ 16KB

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

### 4.4 مثال تكوين Caddy:
```caddy
fcm-admin.example.com {
    request_body {
        max_size 16KB
    }
    reverse_proxy 127.0.0.1:8080 {
        header_up Host {host}
        header_up X-Real-IP {remote_host}
        header_up X-Forwarded-For {remote_host}
        header_up X-Forwarded-Proto {scheme}
    }
}
```

---

## 5. النسخ الاحتياطي لقاعدة البيانات (`data.db`)

تحتوي قاعدة بيانات SQLite (`data.db`) على تكوينات التطبيقات، وحسابات المشرفين، وسجل الأجهزة،
ومفاتيح التطبيقات المشفرة.
تعمل قاعدة البيانات بنمط `WAL` (Write-Ahead Logging)، لذا يجب استخدام الأمر المدمج `sqlite3 .backup` لضمان أخذ نسخة متسقة دون إيقاف الخدمة:

```bash
# أمر النسخ الاحتياطي الآمن أثناء تشغيل الخادم
sqlite3 /data/data.db ".backup /backups/fcm_backup_$(date +%Y%m%d_%H%M%S).db"
```

### جدولة النسخ التلقائي (Cron):
أضف سطراً في `crontab` على خادم التشغيل لتشغيل النسخ يومياً في الساعة 03:00 فجراً:
```cron
0 3 * * * sqlite3 /path/to/data/data.db ".backup /path/to/backups/fcm_$(date +\%F).db" && find /path/to/backups/ -name "fcm_*.db" -mtime +14 -delete
```

### اختبار الاستعادة (Restore Test):
للتحقق من سلامة النسخة الاحتياطية:
```bash
sqlite3 /backups/fcm_test.db "PRAGMA integrity_check;"
# يجب أن يطبع: ok
```

---

## 6. إجراءات التحديث والتراجع (R1 - Upgrade & Rollback)

### 6.1 التحديث (Upgrade):
1. أخذ نسخة احتياطية فورية من `data.db`:
   `sqlite3 data.db ".backup pre_upgrade.db"`
2. تدوين إصدار الـ commit الحالي العامل:
   `git rev-parse HEAD`
3. سحب الصورة الجديدة أو إعادة البناء:
   `docker compose pull && docker compose up -d`
4. متابعة سجلات التشغيل للتأكد من نجاح التهجيرات التلقائية (Migrations):
   `docker compose logs -f fcm_microservice`

### 6.2 التراجع في حال الطوارئ (Rollback):
1. إيقاف الحاوية:
   `docker compose stop fcm_microservice`
2. استعادة نسخة قاعدة البيانات السابقة:
   `cp pre_upgrade.db /data/data.db`
3. إعادة تشغيل الصورة السابقة في `docker-compose.yml`.
4. إعادة تشغيل الخدمة والتحقق من الاستقرار.

---

## 7. الكلفة المعروفة ومخبأ الأدوار (Role Cache Evaluation)

وفق نتائج التقييم في `T13A-report.md`:
- يعتمد مخبأ الأدوار على بصمة فريدة `role_cache_fingerprint` محسوبة كـ `SHA-256(identity_json + data_source_json)`.
- أي تعديل في إعدادات قسم الهوية أو مصدر البيانات في الداشبورد يؤدي تلقائياً إلى تغيير البصمة، مما يبطل كافة الإدخالات القديمة في المخبأ فوراً وبأمان تام.
- التطبيقات التي لا تمتلك إعدادات تعتمد بصمة ثابتة `LEGACY_FINGERPRINT = "legacy"`.

---

## 8. ملاحظة معالجة حقل `iat` في توكنات JWT

> [!NOTE]
> في مزود هوية JWT (`src/identity/jwt.rs:188`)، لا يُطبَّق شرط أقصى عمر للتوكن `max_age_seconds`
> إلا إذا احتوى التوكن على حقل `iat` (Issued At).
> إذا كان الباك إند الخاص بك لا يُدرج `iat` في حمولة JWT، فسيعتمد الخادم على فحص انتهاء الصلاحية `exp` فقط
> ولن يفحص عمر التوكن. لذلك يُنصح دائماً بإدراج `iat` في جميع توكنات JWT الصادرة من الباك إند.

---

## 9. مراقبة وإيقاف المفتاح القديم (R4 وR5)

1. أثناء فترة الانتقال، راقب استخدام المفتاح القديم في السجلات اليومية (يبحث عن تنبيهات `legacy key used`).
2. بمجرد تحديث كافة تطبيقات الهواتف للمسارات الجديدة، قم بإطفاء المفتاح القديم في `.env`:
   ```bash
   LEGACY_API_KEY_ENABLED=false
   ```
3. أعد تشغيل الخدمة، وتحقق أن استدعاء `/api/v1/send` بالمفتاح القديم يرد بـ **`401 Unauthorized`**.

---

## 10. تدوير المفاتيح وما يُحذف عند حذف تطبيق

### 10.1 تدوير مفتاح الخدمة ومفاتيح التطبيقات
- **`SERVICE_API_KEY`:** يُنشأ خارج المستودع بـ 32 بايت عشوائي، ويُحدّث في `.env` مع إعادة تشغيل الخادم.
- **مفاتيح التطبيقات (`app_keys`):** تُنشأ من الداشبورد لكل تطبيق. لإلغاء أي مفتاح، اضغط على زر "إلغاء" بجانب المفتاح ليتم تعطيله فوراً دون الحاجة لإعادة تشغيل.

### 10.2 ما يُحذف وما يبقى عند حذف تطبيق (`DELETE /admin/apps/:app`)
عند قيام المالك بحذف تطبيق:
- **ما يُحذف فوراً:** ملف الاعتماد `credentials/<app>.json`، وسجل إعدادات التطبيق `app_configs`، ومفاتيح التطبيق `app_keys`، وسجل أجهزة التطبيق `devices`.
- **ما يبقى في قاعدة البيانات:** سجل الإرسال التاريخي `notifications_history`، وسجل الحملات `campaigns`، وسجل التدقيق الإداري `audit_log` (مع تدوين نسخة من إعدادات التطبيق في سجل التدقيق قبل حذفه لضمان سلامة التدقيق المالي والإداري).

---

## 11. تحقق منه

- **فحص التحقق من متغيرات البيئة:** `cargo test main::tests::cors_*` في `src/main.rs`.
- **فحص حد محاولات تسجيل الدخول:** اختبار `admin::tests::sixth_failed_login_returns_429` في `src/admin/mod.rs`.
- **فحص أمان الكوكي:** اختبارات `admin::session::tests::set_cookie_*` في `src/admin/session.rs`.
- **فحص قيود حذف التطبيق:** اختبارات `admin::credentials::tests::delete_app_*` في `src/admin/credentials.rs`.

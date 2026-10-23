# جدول مسارات T13-A

هذا الملف هو القائمة الثابتة التي يقارن بها الاختبار `route_table_is_documented`
جدول المسارات المولَّد من الكود (`public_routes()` و`admin_routes()`). أي مسار
يُضاف إلى الـRouter بلا سطر هنا يُفشل الاختبار.

الأعمدة هي: الطريقة، المسار الكامل (مع بادئة `/api/v1` أو `/admin`)، نوع
المصادقة، ثم تفصيل الأدوار لمسارات لوحة التحكم. الاختبار يقارن الأعمدة الثلاثة
الأولى حرفياً؛ عمود الأدوار توثيقي لأنه مفروض داخل كل معالج (`Need::Owner` أو
`Need::AnyMember`) وليس في جدول المسارات نفسه.

المصادقة:

- `service key or admin user`: مفتاح الخدمة أو توكن مستخدم بدور إداري.
- `service key, application key of the app or admin user`: مفتاح الخدمة أو مفتاح
  التطبيق نفسه أو توكن مستخدم إداري.
- `service key, application key of the app or user token`: مثل السابق بلا شرط
  الدور الإداري.
- `service key, application key of the app, user token or legacy key`: المسار
  الوحيد الذي يقبل المفتاح القديم (`/send`).
- `user token, or nothing for a public topic`: توكن مستخدم، أو بلا مصادقة
  للموضوعات العامة.
- `user token`: توكن مستخدم فقط.
- `dashboard session`: جلسة لوحة التحكم (الكوكي)، والدور كما في العمود الأخير.
- `none`: بلا جلسة (تسجيل الدخول والخروج فقط).

```routes
GET | /api/v1/apps | service key or admin user | -
GET | /api/v1/history | service key, application key of the app or admin user | -
GET | /api/v1/topics | service key, application key of the app or admin user | -
POST | /api/v1/topics | service key, application key of the app or admin user | -
POST | /api/v1/send | service key, application key of the app, user token or legacy key | -
POST | /api/v1/send/template | service key, application key of the app or user token | -
GET | /api/v1/campaigns | service key, application key of the app or user token | -
POST | /api/v1/campaigns | service key, application key of the app or user token | -
POST | /api/v1/campaigns/:id/cancel | user token | -
POST | /api/v1/topics/subscribe | user token, or nothing for a public topic | -
POST | /api/v1/topics/unsubscribe | user token, or nothing for a public topic | -
POST | /api/v1/devices/register | user token | -
POST | /api/v1/devices/unregister | user token | -
POST | /admin/auth/login | none | -
POST | /admin/auth/logout | none | -
GET | /admin/auth/me | dashboard session | owner, manager
POST | /admin/auth/password | dashboard session | owner, manager
POST | /admin/auth/reauth | dashboard session | owner, manager
GET | /admin/apps | dashboard session | owner, manager (manager: own apps)
GET | /admin/apps/:app | dashboard session | owner, manager (bound apps)
POST | /admin/apps/:app/init | dashboard session | owner
PUT | /admin/apps/:app/:section | dashboard session | owner, manager (owner for identity, data_source, devices, app_check)
GET | /admin/apps/:app/templates | dashboard session | owner, manager
POST | /admin/apps/:app/templates | dashboard session | owner, manager
POST | /admin/apps/:app/templates/validate | dashboard session | owner, manager
GET | /admin/apps/:app/templates/:key | dashboard session | owner, manager
PUT | /admin/apps/:app/templates/:key | dashboard session | owner, manager
DELETE | /admin/apps/:app/templates/:key | dashboard session | owner, manager
PUT | /admin/apps/:app/templates/:key/enabled | dashboard session | owner, manager
POST | /admin/apps/:app/templates/:key/preview | dashboard session | owner, manager
POST | /admin/apps/:app/templates/:key/test-send | dashboard session | owner, manager
GET | /admin/apps/:app/versions | dashboard session | owner, manager
GET | /admin/apps/:app/versions/:id | dashboard session | owner, manager
POST | /admin/apps/:app/versions/:id/restore | dashboard session | owner, manager
GET | /admin/apps/:app/export | dashboard session | owner
POST | /admin/apps/:app/import | dashboard session | owner
GET | /admin/audit | dashboard session | owner, manager (manager: own apps)
GET | /admin/apps/:app/campaigns | dashboard session | owner, manager
PUT | /admin/apps/:app/campaigns | dashboard session | owner, manager
POST | /admin/apps/:app/campaigns/:id/approve | dashboard session | owner, manager
POST | /admin/apps/:app/campaigns/:id/reject | dashboard session | owner, manager
GET | /admin/apps/:app/blocks | dashboard session | owner, manager
POST | /admin/apps/:app/blocks | dashboard session | owner, manager
DELETE | /admin/apps/:app/blocks/:entity_id | dashboard session | owner, manager
GET | /admin/apps/:app/history | dashboard session | owner, manager
GET | /admin/apps/:app/history/:id | dashboard session | owner, manager
POST | /admin/apps/:app/send | dashboard session | owner, manager
GET | /admin/apps/:app/devices/stats | dashboard session | owner, manager
GET | /admin/apps/:app/devices | dashboard session | owner, manager
GET | /admin/apps/:app/devices/:id/topics | dashboard session | owner, manager
GET | /admin/users | dashboard session | owner
POST | /admin/users | dashboard session | owner
PUT | /admin/users/:id | dashboard session | owner
POST | /admin/users/:id/reset-password | dashboard session | owner
DELETE | /admin/users/:id | dashboard session | owner
PUT | /admin/auth/test-uid | dashboard session | owner, manager
POST | /admin/apps/:app/identity/test | dashboard session | owner
POST | /admin/apps/:app/data_source/test | dashboard session | owner
GET | /admin/apps/:app/keys | dashboard session | owner
POST | /admin/apps/:app/keys | dashboard session | owner
DELETE | /admin/apps/:app/keys/:id | dashboard session | owner
POST | /admin/apps | dashboard session | owner
PUT | /admin/apps/:app/credentials | dashboard session | owner
GET | /admin/apps/:app/credentials | dashboard session | owner
POST | /admin/apps/:app/disable | dashboard session | owner
POST | /admin/apps/:app/enable | dashboard session | owner
DELETE | /admin/apps/:app | dashboard session | owner
```

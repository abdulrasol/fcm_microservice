# تقرير T20 — تسجيل سبب رفض إرسال القالب

## المشكلة
`POST /api/v1/send/template` كان يرجع 400/403/404/503 بدون أي سطر في اللوك، فلا يمكن معرفة سبب عدم وصول إشعارات التسجيل وتغيير الدور.

## ما تغير (`src/main.rs`)
- دالتان: `log_field` (تنظّف الأحرف التحكمية وتقصّ إلى 64 حرفاً) و`reject_template` (تسجل `warn` وترجع الخطأ كما هو).
- كل مخرج مبكر في `send_template` يسجل: `app, template, sender, status, reason`.
- الأسباب: unknown_app, ip_limited, unauthenticated_service_key, identity_rejected_or_unavailable, role_lookup_unavailable, app_config_missing, template_not_found_or_disabled, role_not_listed(role=...), rate_limited, invalid_param, context_*, render_error, validation_failed, dedupe_error, delivery_error.
- عند 400 يُضاف `detail=` (أول 120 حرفاً من الرسالة).
- `warn` عند جمهور فارغ (`no_devices`) و`info` عند نجاح الإرسال.
- ردود العميل لم تتغير.

## الاختبارات (4 جديدة)
- role_not_listed، المعامل الفارغ، القالب غير الموجود، الجمهور الفارغ.
- التأكد أن اللوك لا يحتوي قيمة معامل ولا توكن.
- تجربة سلبية: حذف سطر اللوك لـ role_not_listed جعل الاختبار يفشل، ثم أُعيد.

## النتيجة
`cargo test`: 692 نجحت. `clippy -D warnings` و`fmt --check` نظيفان.

## بعد النشر
جرّب التسجيل وتغيير الدور ثم:
`docker logs --since 10m fcm_microservice 2>&1 | grep "template"`

## التوصيل المؤجل — توثيق مواضع الاستدعاء

تم تأجيل توحيد `trusted_sender` في T13-A-2 (وثيقة T13-A-2 §3):
- `send_template` (~src/main.rs:1067): `error.into_response()`
- `authorize_campaign_sender` (~src/main.rs:1533): `auth_error_with_limit()` (يحمل `Retry-After` للمحددات)
- `authenticate_admin` و`authenticate_send` (`src/auth.rs`): لا يُعيدان `Response` مباشرة، بل `AuthError` تُحوَّل لاحقاً.

الحل المقترح لـ T13-B: طبقة `trusted_sender` تُعيد `(StatusCode, HeaderMap, String)` دائماً؛ `authorize_campaign_sender` يُعيد `Response` بدلاً من `AdminError`. لم يُنفَّذ هنا لتجنب تغيير سلوك الاستجابة في مسار الحملات قبل مراجعة واجهة T13-B (dashboard wiring).

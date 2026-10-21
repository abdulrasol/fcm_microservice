# آلية التنفيذ: منفذ + مراجع

الخطة: `docs/plans/secure-sending-plan.md` (المرجع الوحيد لما يجب بناؤه).

## الأدوار
- **المنفذ** (نموذج آخر): ينفذ ملف الخطوة حرفياً ويكتب تقريراً.
- **المراجع** (Claude في جلسة التخطيط): يقرأ التقرير، ويتحقق بنفسه من الكود والـ git، ثم يقرر: ✅ التالي، أو 🔧 إصلاحات (ملف `Sx-fix-N.md`).
- **المالك**: يشغّل المنفذ، ويعطي المراجع التقرير، ويقرر ما يخرج عن الخطة.

## الدورة لكل خطوة
1. المراجع يكتب `docs/plans/execution/<ID>.md` (أمر كامل ومحدد).
2. المالك يشغّل المنفذ بالجملة: `نفّذ docs/plans/execution/<ID>.md حرفياً، واقرأ docs/plans/execution/RULES.md أولاً.`
3. المنفذ يكتب `docs/plans/reports/<ID>-report.md` بالقالب في RULES.md.
4. المالك يخبر المراجع: "خلص <ID>".
5. المراجع يتحقق ثم: ✅ يكتب أمر الخطوة التالية، أو 🔧 يكتب `<ID>-fix-1.md` (ما يجب إصلاحه فقط).
6. لا تبدأ خطوة قبل ✅ على التي قبلها.

## الترتيب
S0 ← T1 ← T2 ← T3 ← T4 ← T5 ← T6 ← T7 ← T8 ← T9 ← T10 ← T11 ← T12 ← T16 ← T13 ← (T14, T15 اختيارية)، ثم مهام واط W1..W8 بعد نشر الأداة.
(قد يعدّل المراجع الترتيب إذا ظهر اعتماد جديد، ويدوّن السبب هنا.)

## سجل الخطوات
| الخطوة | الحالة | ملاحظة |
|---|---|---|
| S0 | ⬜ أمر جاهز | |
| T1 | ✅ | مخزن الإعدادات والقوالب: structs + تحقق + SQLite + إصدارات + استيراد/تصدير. بلا ربط بالطلبات الحية — **راجع T2-0** (إصلاحات القفل و`last_used_at` و`[G6]`) |
| T2 | ✅ | حسابات الداشبورد والجلسات: `tool_users`/`tool_sessions` + argon2id + كوكي + CSRF + `create-owner`/`reset-owner-password`. القرارات: CORS يُحل بـ proxy في Vite ضمن T12 ولا يُعدَّل الآن؛ كود الخطأ `internal` معتمد. التقرير: `docs/plans/reports/T2-report.md` |
| T3 | ✅ | مزودات الهوية: نقل JWKS/Firebase من `auth.rs` إلى `src/identity/` + مزود JWT عام (JWKS/HS256) + سجل لكل تطبيق، **بلا ربط بمسارات الإرسال**. التقرير: `docs/plans/reports/T3-report.md`. قرار: حقلا `identity` و`datasource` يُضافان إلى `AppState` في T5 مع السماح بتعديل سطر واحد في `test_state()` داخل `src/admin/mod.rs` |
| T4 | ✅ | مصادر البيانات: واجهة `DataSource` (`get_doc`/`members`) + تحقق المسارات والحقول والترميز + مصدر Firestore معمَّم + resolver HTTP موقّع HMAC + مثال FastAPI + سجل لكل `(تطبيق، إصدار)`، **بلا ربط بمسارات الإرسال وبلا إضافة إلى `AppState`**. التقرير: `docs/plans/reports/T4-report.md`. اعتمده المراجع بقراره. |
| T5 | ✅ | تعميم الأدوار والاشتراك: `src/roles.rs` (محلّل + مخبأ + أنماط) + `AuthService` فوق `IdentityRegistry`/`DataSourceRegistry`/`ConfigStore` + `web_subscribe` + `LEGACY_SIGNED_IN_TOPICS` للتطبيقات بلا إعداد + حقلا `identity`/`datasource` في `AppState`. 212 اختباراً ناجحاً. قرار المراجع: `#[allow(dead_code)]` المؤقتة مقبولة وتزول عند استعمالها، وتعديلات اختبارات Firebase مقبولة. التقرير: `docs/plans/reports/T5-report.md` |
| T6 | ✅ | سجل الأجهزة: جدول `devices` + `DeviceStore` (تسجيل/نقل ملكية/إخلاء الأقدم/إلغاء) + `RoleDirectory` ومخبأ `(app, version, role)` + `tokens_for_role` + `POST /api/v1/devices/register` و`/devices/unregister` بهوية مستخدم فقط وحد 20/دقيقة لكل uid. 242 اختباراً ناجحاً (30 جديداً). اعتمده المراجع بقراره. التقرير: `docs/plans/reports/T6-report.md` |
| T7 | ✅ | محرك القوالب: `segments()` في `config/placeholder.rs` (إضافة صرفة) + `src/template.rs` (`validate_params`/`context_path`/`render`) بلا شبكة ولا قاعدة بيانات. 285 اختباراً ناجحاً (43 جديداً). اعتمده المراجع بقراره. التقرير: `docs/plans/reports/T7-report.md` |
| T8 | ✅ | إرسال القوالب: `src/send.rs` (`FcmTransport`/`HttpFcm`/`build_message`/`send_to_tokens`) + `POST /api/v1/send/template` بالترتيب 12 خطوة، و`/send` القديم بنفس الردود حرفياً (4 اختبارات توصيف). `db.rs`: `kind`/`ref` في السجل + `template_dedupe` (hash + ذرّية) + `touch_template_used` + `TemplateRateLimiter`. اعتمده المراجع بعد إصلاح T8-F (تحرير مفتاح dedupe عند كل خروج بلا تسليم). 335 اختباراً ناجحاً (50 جديداً). التقرير: `docs/plans/reports/T8-report.md` وتقرير الإصلاح: `docs/plans/reports/T8-fix-report.md` |
| T9 | ✅ | حملات الشركات: `src/campaigns.rs` (`build_condition` من مجموعات topics، قواعد المحتوى والروابط والصورة والإجراء، `resolve_entity`، `check_action`، `dispatch_campaign` القابلة لإعادة الاستعمال) + `db.rs` (`campaigns`/`campaign_blocks` بصيغة 3.14، `insert_campaign_checked` بحصة داخل معاملة وقفل كتابة، `review.mode`، `claim_for_sending`/`cancel_pending`/`finish_campaign`/`is_entity_blocked`) + `template.rs` (Snippet/`render_snippet`/`validate_param_map`/`check_require_map` بلا تعديل اختبارات T7) + `POST|GET /api/v1/campaigns` و`POST /campaigns/{id}/cancel`. 394 اختباراً ناجحاً (59 جديداً). التقرير: `docs/plans/reports/T9-report.md` |
| T10 | ✅ | واجهات الداشبورد مقسمة إلى ثلاث شرائح: **A** = التطبيقات والإعدادات والقوالب والإصدارات والاستيراد/التصدير والتدقيق (✅ بقرار المراجع)، **B** = الحملات (موافقة/رفض/حظر) والسجل والإرسال الحر وإحصاءات الأجهزة وتعافي الحملات العالقة (منفذة، 🔄 بانتظار المراجعة)، **C** = حسابات الأداة وفحوص `identity/test` و`data_source/test` (متبقية). |
| T10-A | ✅ | تطبيقات وإعدادات وقوالب وإصدارات واستيراد/تصدير وتدقيق: `src/admin/apps.rs` و`src/admin/templates.rs` + `authorize` في `mod.rs` + قراءات في `db.rs` + قاعدة `context.require` في `validate.rs`. 450 اختباراً ناجحاً (56 جديداً)، clippy 0، والتجارب الخمس موثقة. التقرير: `docs/plans/reports/T10A-report.md` |
| T10-B | ✅ | حملات الداشبورد والسجل والإرسال الحر وإحصاءات الأجهزة وتعافي الحملات العالقة: `src/admin/campaigns.rs` (قائمة/موافقة/رفض/حظر بانتقال ذري) و`src/admin/history.rs` (السجل/الإرسال الحر/إحصاءات الأجهزة) + `ConfigStore::app_names` + `db::fail_stale_sending` واستدعاؤه عند الإقلاع. 497 اختباراً ناجحاً (47 جديداً)، clippy 0، والتجارب الخمس موثقة. التقرير: `docs/plans/reports/T10B-report.md` |
| T10-C | 🔄 بانتظار المراجعة | حسابات الأداة وفحوص الاتصال و`test_uid`: `src/admin/users.rs` (قائمة/إنشاء/تعديل/إعادة تعيين/حذف، owner فقط، وكلمة مرور مؤقتة تظهر مرة واحدة مع `Cache-Control: no-store`) و`src/admin/diagnostics.rs` (`identity/test` و`data_source/test`، بلا قيم الـclaims ولا قيم الوثيقة) + `PUT /auth/test-uid` + إصلاح سباق الحظر في `claim_for_sending` (`NOT EXISTS` ذرّي). 538 اختباراً ناجحاً (41 جديداً)، clippy 0، والتجارب الخمس موثقة. التقرير: `docs/plans/reports/T10C-report.md` |
| T11 | 🔄 بانتظار المراجعة | مفاتيح التطبيقات: `src/appkeys.rs` (`AppKeyStore`: `fcm_<app>_<random>`، sha256 فقط، حد 20 لكل تطبيق، إلغاء فوري) + جدول `app_keys` + `trusted_sender` في `src/auth.rs` (مفتاح التطبيق في المسارات التي تحمل `app` فقط) + `src/admin/keys.rs` (owner فقط، المفتاح يظهر مرة واحدة). 578 اختباراً ناجحاً (39 جديداً). التقرير: `docs/plans/reports/T11-report.md` |

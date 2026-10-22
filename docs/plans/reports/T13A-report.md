# تقرير T13-A (قيد التنفيذ — A فقط)
- التاريخ: 2026-10-22 20:23
- الفرع: feature/policy-auth؛ HEAD قبل/بعد: a54e817 → 145ea24
- الأوامر: cargo fmt / clippy 0 / cargo test 666 passed (55 s) / 6 تجارب سلبية (5 فشلت الهدف + 2 إضافية للتوصيل المؤجل)

## 1. ما نُفِّذ (بند بند)
| بند | الحالة | ملاحظة |
|---|---|---|
| T13-A-1 | ✅ | legacy budget + env فقط في main.rs |
| T13-A-2 | ✅ | build_router + 9 e2e |
| T13-A-3 | ✅ | AdminLimiters (test/free/admin/credentials) |
| T13-A-4 | ✅ | restore Locked ثنائي الشكل |
| T13-A-5 | ✅ | require موحد + role_cache_fingerprint |
| T13-A-6 | ✅ | history filters + audit manager + campaign_quota |
| T13-A-7 | ✅ | route_table_is_documented + T13A-routes.md |

## 2. الملفات (بعد-last-commit diff --stat من base 60024e2)
`src/admin/*.rs` + `src/main.rs` + `src/config/store.rs` + `src/legacy.rs` + `src/e2e.rs` + `docs/plans/reports/T13A-routes.md`. لم يمس `Cargo.toml` / `dashboard/` / `.env` (عدا `.env.example` في T13-A-1).

## 3. الأوامر والنتائج
- cargo fmt: OK
- cargo clippy --all-targets: 0 تحذيرات جديدة
- cargo test: 666 passed; 0 failed; 0 ignored
- التجارب الست (6): 5 فشلت الهدف + التجربة 2 (test_send) + 2b (free_send) أُجريت إضافياً بسبب التوصيل المؤجل؛ جميعها استعادت sha256 مطابق.

## 4. التحقق من البنود
- legacy_rate_limited: 429 + Retry-After: 60 (main.rs ~455)
- admin_routes + public_routes: جدول مسجل في T13A-routes.md؛ الاختبار يفشل إذا أُضيف مسار بلا سطر
- AdminLimiters: test_send 10/min، free_send 10/min، admin_writes 30/min، credentials 10/min (main.rs 76)
- restore: `ConfigDocument` أولاً، ثم `AppConfig` (store.rs 669)
- require: `Ref::Param | Ref::Sender` فقط (validate.rs 557)
- role_cache_fingerprint: SHA-256(identity + data_source)؛ `LEGACY_FINGERPRINT = "legacy"`
- history: `ref` ≤100، `since`/`until` RFC3339 أو YYYY-MM-DD؛ SQL `datetime(created_at)`
- audit: manager بدون app ⇒ فقط `user.apps` (apps.rs 368)؛ foreign app ⇒ 403
- campaigns: `company_quota_remaining` + `quota_usage` عامة (db)
- لا count مستلمين في POST /send (مُوثَّق)

## 5. انحرافات / توثيق
- trusted_sender: مُؤجَّل (call sites: send_template ~1067 `into_response()`، authorize_campaign_sender ~1533 `auth_error_with_limit()`). لم يُعدَّل؛ موثَّق هنا لي انتقل إلى T13-B.
- `admin_writes` يستخدم `check_write_limit` (users 54 / diagnostics 37 / apps 173) — كلهم على `admin_limiters.admin_writes`، لا يُستعير `auth_fail`/`topics`/`devices`.
- التجارب 2/2b أُجريت بصيغة أكثر صرامة (مفتاح التوصيل المؤجل) بسبب التوصيل القديم؛ السجلات محفوظة.

## 6. ملاحظات خارج النطاق
- `.DS_Store` (جذر) — لم يُنشأ من المُنفيذ؛ لم يُحذف ولا أُضيف.
- `cargo test` كامل يأخذ 55–120 s حسب التحميل؛ لم أُغيِّر `--test-threads` في الالتزام.

## 7. أسئلة للمالك/المراجع
- هل نبدأ T13-B مباشرة بعد الموافقة على الرموز (docs + dashboard wiring لـ ref/since/until + company_quota_remaining)؟
- هل يُريد توثيق `trusted_sender` في ملف منفصل قبل التوحيد (مقترح: `docs/plans/reports/T13A-trusted_sender.md`)؟

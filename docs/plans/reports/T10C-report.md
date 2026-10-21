# تقرير T10-C

- **التاريخ/الوقت:** 2026-10-22، من 00:35 إلى 01:05 (توقيت الجهاز).
- **الفرع:** `feature/policy-auth` (تحقّق قبل البدء وقبل كل commit؛ لم يتغيّر).
- **HEAD قبل العمل:** `1084dad` (`docs(plan): T10B update execution tracker`).
- **HEAD بعد العمل:** آخر commit هو `docs(plan): T10C update execution tracker` — 4 commits جديدة (3 كود + 1 توثيق)، بلا push.
- **ملف الخطوة المنفَّذ:** `docs/plans/execution/T10C.md` (الشريحة C والأخيرة من T10).

## 1. ما نُفِّذ (بند بند من ملف الخطوة)

| البند | الحالة | ملاحظة |
|---|---|---|
| §0 التعديلات المسموحة | تم | جديد: `src/admin/users.rs`، `src/admin/diagnostics.rs`. تعديل: `src/admin/mod.rs` (تسجيل المسارات + **تحديث اختبار المصفوفة**، انظر §5)، `src/admin/auth.rs` (إضافة `PUT /auth/test-uid` ومساعد واحد فقط)، `src/db.rs` (قراءة/كتابة حسابات + إصلاح الفقرة 1)، `src/admin/campaigns.rs` (الفقرة 1 فقط)، `docs/plans/**`. **`src/cli.rs` لم يُمَس**: `normalize_email` و`generate_temp_password` كانا `pub` أصلاً، فلم يحتج تغيير رؤية ولا سلوك (§5). ممنوع: بقية `src/` و`Cargo.toml` و`dashboard/` — لم يُمَس. |
| §1 إصلاح سباق الحظر | تم | `db::claim_for_sending` صار يحوي `AND NOT EXISTS (SELECT 1 FROM campaign_blocks b WHERE b.app = campaigns.app AND b.entity_id = campaigns.entity_id)` في نفس `UPDATE`، ومعه `reviewer` و`decided_at` و`review_note`. وفي `approve`: عند فشل الانتقال يُعاد فحص الحظر ⇒ 409 `entity_blocked`، وإلا 409 `not_pending` (فحص الحظر المسبق باقٍ). اختبارات: `claim_for_sending_refuses_blocked_entity` (db) و`approve_when_block_lands_before_claim_is_409_entity_blocked` (الحظر يُحاقن بين الفحصين باستدعاء `claim_for_sending` مباشرة). |
| §2 `GET /users` | تم | `[{id, email, role, apps, disabled, must_change_password, test_uid, created_at}]` مرتبة بالمعرّف. **لا hash**: الاختبار `users_list_hides_hash_and_is_owner_only` يفشل إذا ظهر `password_hash` أو `$argon2`. |
| §2 `POST /users` | تم | owner فقط (`require_owner` أول سطر في المعالج). جسم `{email, apps}` بـ`deny_unknown_fields` ⇒ `role` في الجسم ⇒ 400. يُنشئ **مديراً فقط** (`role='manager'` في قاعدة البيانات). التحقق: بريد بتطبيع الـCLI، مكرر ⇒ 409 `{"error":"duplicate_email"}` (غير حساس للحالة)، `apps` غير فارغة (وإلا 400)، كل تطبيق موجود في `project_map` أو `config.app_names()` (وإلا 400)، 50 تطبيقاً كحد أقصى. `require_reauth(300)` ⇒ 403 `reauth_required`. الرد `201 {"id","email","temporary_password"}` مع `Cache-Control: no-store`. تدقيق `user.create` (id وemail والتطبيقات). |
| §2 `PUT /users/{id}` | تم | جسم `{apps?, disabled?}`؛ هدف مالك ⇒ 403 `owners are managed with the CLI`؛ غير موجود ⇒ 404؛ المالك لا يعدّل نفسه (نفس 403). تغيير `apps` يستبدل المجموعة في معاملة واحدة (حذف + إدراج) وبنفس التحقق، **ويحذف كل جلسات المستخدم**؛ `disabled=true` يحذف الجلسات أيضاً و`false` يعيد التفعيل. تدقيق `user.update`. |
| §2 `POST /users/{id}/reset-password` | تم | مدير فقط، `require_reauth(300)`، كلمة مرور مؤقتة جديدة، `must_change_password=1`، حذف كل الجلسات، الرد `201` نفسه مع `no-store`. تدقيق `user.reset_password`. |
| §2 `DELETE /users/{id}` | تم | مدير فقط؛ لا يحذف نفسه ولا مالكاً (403)؛ الحساب والجلسات وروابط التطبيقات تُحذف (cascade، ويتحقق منه `delete_user_cascades_apps_and_sessions`). تدقيق `user.delete`. |
| §2 `PUT /auth/test-uid` | تم | أي مستخدم مسجّل، على نفسه فقط: الجسم `{test_uid}` سلسلة أو `null`، التحقق بـ`crate::identity::is_valid_uid` (دالة `pub(crate)` أصلاً، فُحصِّرت بلا تغيير)، ويحدّث صفّ المستخدم **من الجلسة فقط** (أي `user_id` في الجسم ⇒ 400). تدقيق `user.test_uid` (بلا قيمة الـuid). `/auth/me` لا يعرض `test_uid` (اختبار `me_still_hides_test_uid`). |
| §2 حد الكتابة 20/دقيقة | تم | `state.devices_limiter` (المعدّل الموجود 20/دقيقة) بمفتاح `admin:users:<id>`. |
| §3 `POST /apps/{app}/identity/test` | تم | owner فقط، حد 20/دقيقة (المعدّل الموجود، مفتاح `admin:diagnostics:<id>`). جسم `{token}` ≤ 8192 بايت (وإلا 400). التحقق بمزود التطبيق المحفوظ `state.identity.provider_for(app)`. النجاح ⇒ `{ok:true, uid, name?, role, is_admin, claim_names:[...]}` — **أسماء الـclaims فقط**؛ `IdentityError::Invalid` ⇒ `200 {ok:false, reason}`؛ `IdentityError::Unavailable` ⇒ 503. **التوكن لا يظهر في الرد ولا في التدقيق ولا في السجل** (اختبار `identity_test_never_echoes_token`). تدقيق `identity.test` بلا تفاصيل. |
| §3 `POST /apps/{app}/data_source/test` | تم | جسم `{path?}` أو `{members:{collection,field,value,limit?}}` **واحد بالضبط** (وإلا 400). `path` ⇒ `get_doc` ⇒ `{ok:true, found, fields:[{name,type}]}` (أسماء الحقول العليا وأنواعها، بلا قيم)؛ `members` ⇒ `{ok:true, count}` (العدد فقط، limit افتراضي 50 وأقصى 200). `InvalidPath` ⇒ 400؛ `Unavailable` ⇒ `200 {ok:false, reason:"data source unavailable"}` (رسالة عامة). تدقيق `data_source.test`. |
| §3 التطبيق يجب أن يكون معروفاً | تم | كلا الفحصين: 404 إن لم يكن للتطبيق إعداد ولا credentials. |
| §4 الاختبارات (41 اسماً) | تم | كل الأسماء المطلوبة موجودة وتعمل (فحص آلي)، زيادةً على `matrix_includes_users_and_diagnostics_routes` المذكور في §4. العدد الكلي 538 (497 قبل + 41 جديد). |
| §5 الجودة | تم | `cargo build` بلا تحذيرات، `cargo clippy --all-targets` صفر تحذير، `cargo test` = 538 ناجح/0 فاشل/0 ignored، `rustfmt --check` نظيف. |
| §5 التجارب السلبية الأربع + تجربة الإصلاح | تم | كل تجربة أخفقت الاختبار المحدد ثم أُعيد الملف بنفس `sha256` (§3). |
| §6 الـ commits | تم | (1) `fix(campaigns): T10C-1 refuse approval of a blocked entity atomically`، (2) `feat(admin): T10C-2 add tool account routes and test uid`، (3) `feat(admin): T10C-3 add identity and data source test routes`، (4) `docs(plan): T10C update execution tracker`. |

## 2. الملفات

`git diff --stat 1084dad..HEAD -- src/`:

```
 src/admin/auth.rs        |   37 ++
 src/admin/campaigns.rs   |   40 ++
 src/admin/diagnostics.rs |  642 +++++++++++++++++++++++++++++++++++++++++++++
 src/admin/mod.rs         |  160 +++++++-
 src/admin/users.rs       | 1019 ++++++++++++++++++++++++++++++++++++++++++++++
 src/db.rs                |  229 ++++++++++-
 6 files changed, 2120 insertions(+), 7 deletions(-)
```

- **`src/admin/users.rs` (جديد):** حسابات الأداة: `list_users`، `create_user`، `update_user`، `reset_password`، `delete_user` + `check_write_limit` (مفتاح `admin:users:<id>`) + `validate_apps` + `UserAdminError` (لجسم 409 `{"error":"duplicate_email"}`) + 27 اختباراً.
- **`src/admin/diagnostics.rs` (جديد):** `identity_test` و`data_source_test` + `field_types`/`value_type` + 11 اختباراً.
- **`src/admin/auth.rs` (تعديل):** `PUT /auth/test-uid` فقط: `TestUidRequest` (بـ`deny_unknown_fields`) و`set_test_uid` (يقرأ `user_id` من الجلسة لا من الجسم).
- **`src/admin/mod.rs` (تعديل):** `pub mod diagnostics;` و`pub mod users;` وتسجيل 8 مسارات جديدة في `admin_routes()`، وتحديث مصفوفة الصلاحيات لتغطي المسارات الجديدة (ومنها إنشاء مدير في مُعِدّ المصفوفة لأن مسارات `/users` تعمل على مدير).
- **`src/admin/campaigns.rs` (تعديل):** إعادة فحص الحظر بعد فشل الانتقال في `approve` + اختبار `approve_when_block_lands_before_claim_is_409_entity_blocked`.
- **`src/db.rs` (تعديل):** قسم `T10-C: dashboard accounts` (قراءة وكتابة الحسابات: `list_accounts`، `get_account`، `account_exists`، `insert_account`، `replace_account_apps`، `delete_account_sessions`، `set_account_password`، `set_account_disabled`، `set_account_test_uid`، `delete_account`) + شرط `NOT EXISTS` في `claim_for_sending` + اختباره.

## 3. الأوامر التي شغلتها ونتائجها

```
$ cargo build
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.66s      # بلا تحذيرات

$ cargo test
   test result: ok. 538 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 92.58s

$ cargo clippy --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.05s      # صفر تحذير

$ rustfmt --check --edition 2021 --config skip_children=true \
      src/admin/mod.rs src/admin/users.rs src/admin/diagnostics.rs \
      src/admin/auth.rs src/admin/campaigns.rs src/db.rs
   (بلا مخرجات)                                                            # نظيف

$ cargo test -- --list | grep -c ': test$'
538
```

**اختبارات الشريحة (41):** `admin::users::tests` 27، `admin::diagnostics::tests` 11، `db::tests::claim_for_sending_refuses_blocked_entity` 1، `admin::campaigns::tests::approve_when_block_lands_before_claim_is_409_entity_blocked` 1، `admin::tests::matrix_includes_users_and_diagnostics_routes` 1.

**التجارب السلبية (§5) — نُفِّذت على الشيفرة النهائية، وأُعيد كل ملف بنفس البصمة:**

| # | التغيير المؤقت | الاختبار | المخرج الفعلي | `sha256` قبل/بعد |
|---|---|---|---|---|
| (أ) | `require_owner` من `POST /users` | `manager_gets_403_on_every_users_route` | FAILED — `POST /users must be refused` (201 بدل 403) | `663fdffdc752ce90cd938fc92bbc88d63b3b7ab1ec046c869308502113d7c595` (متطابق) |
| (ب) | `PUT /users/{id}` لا يحذف الجلسات | `update_user_disable_kills_sessions_and_blocks_login` | FAILED — عدد الجلسات 1 بدل 0 | نفس البصمة (متطابق) |
| (ج) | `identity/test` يعيد قيم الـclaims | `identity_test_ok_returns_role_and_claim_names_only` | FAILED — ظهر حقل `claims` | `d952f8cf160aa1c722e40ab4fdd16f1755d99a3d3694f0d87f4d90500a46152b` (متطابق) |
| (د) | `PUT /auth/test-uid` يقبل `user_id` من الجسم | `test_uid_body_cannot_target_other_user` | FAILED — 200 بدل 400 | `6dbf65b0c5b72597ee16bb92053f14b9be9b31d350968d779549d79fadc1cb69` (متطابق) |
| (هـ) | إزالة `NOT EXISTS` من `claim_for_sending` | `claim_for_sending_refuses_blocked_entity` | FAILED — `a blocked entity must never be claimed` | `faf2b7289f6e1a62355266ab6e4f785728aae5240823d7ea2e043df800361c26` (متطابق) |

**تجربة إضافية (معيار القبول 2):** `grep -rn "temporary_password" src/` ⇒ كل المواضع في `src/admin/users.rs` (بناء الرد + الاختبارات) ولا موضع في `src/db.rs` ولا في `src/admin/auth.rs` ولا في `audit_log`: الكلمة لا تُخزَّن إلا كـhash، ولا تُدقَّق.

## 4. طريقة الفحص: كل بند من "طريقة الفحص" في الخطة/الخطوة ← النتيجة الفعلية وكيف تحققت

| البند | النتيجة | كيف تحققت |
|---|---|---|
| "لا يوجد مسار ينشئ مالكاً" | تم | `POST /users` يكتب `role='manager'` دائماً (`db::insert_account`)، و`role` في الجسم ⇒ 400 (`create_user_rejects_role_field`)، والهدف مالك في `PUT`/`POST reset-password`/`DELETE` ⇒ 403 (`update_owner_target_is_403`، `reset_password_owner_target_is_403`، `delete_self_or_owner_is_403`). |
| "كل مسارات `/users` تبدأ بـ`require_owner`" | تم | `require_owner()` هو أول سطر في `list_users`، `create_user`، `update_user`، `reset_password`، `delete_user` (مراجعة يدوية + `manager_gets_403_on_every_users_route` لكل المسارات الخمسة). |
| "`temporary_password` يظهر في ردّين فقط" | تم | `temporary_password_response` واحدة تبني الردّين (الإنشاء والإعادة تعيين)؛ الاختبار `users_responses_have_no_store_header_where_secret` يتحقق من `Cache-Control: no-store` في كليهما. |
| "ولا يُخزَّن ولا يُسجَّل" | تم | `temp_password_never_in_audit_or_logs_or_list`: الـhash في القاعدة argon2different عن كلمة المرور، ولا كلمة المرور في أي صف `audit_log` ولا في قائمة `/users`. |
| "التوكن لا يظهر في أي رد ولا سجل ولا تدقيق" | تم | `identity_test_never_echoes_token`: الرد لا يحوي التوكن، وصفوف التدقيق (بما فيها `identity.test`) لا تحويه. |
| "أسماء الـclaims فقط، بلا قيمها" | تم | `identity_test_ok_returns_role_and_claim_names_only`: `claim_names` تحمل الأسماء، ولا حقل `claims`، ولا تظهر قيمة بريد الـclaim في الرد. |
| "إحصاءات الحقول بلا قيم" | تم | `data_source_test_path_returns_field_names_and_types_only`: `fields` = الاسم والنوع فقط، وقيمة الوثيقة ("Sara") لا تظهر. |
| "العدد فقط للأعضاء" | تم | `data_source_test_members_returns_count_only`: `count` فقط، ولا `uids` ولا معرّف عضو. |
| "انتقالات الحالة ذرية" (§1) | تم | `claim_for_sending_refuses_blocked_entity` (د) + `approve_when_block_lands_before_claim_is_409_entity_blocked` (لا إرسال، والحملة تبقى `pending_review`). |
| "لا `unwrap/expect` خارج الاختبارات، كل SQL بـbind، لا كلمات مرور/توكنات في `tracing` أو `audit_log`" | تم | مراجعة يدوية + `cargo clippy --all-targets` صفر تحذير؛ كل استعلامات SQL الجديدة بـ`?` للـbind؛ `tracing` في هذا الشريحة لا يطبع شيئاً (لا استدعاء `tracing` جديد). |
| "المصفوفة تغطي المسارات الجديدة تلقائياً" | تم | `matrix_owner_can_reach_every_route` و`matrix_covers_new_routes` يقارنان مجموعة المسارات المسجّلة بمجموعة حالات المصفوفة، و`matrix_includes_users_and_diagnostics_routes` يتحقق من أن المسارات الستة الجديدة مغطّاة. |

## 5. انحرافات عن الأمر (وسببها)

1. **`src/admin/mod.rs` تجاوز "تسجيل المسارات فقط":** Apart from the route table and the two module declarations, the **test module** changed, because the step requires the new routes to be covered by the permission matrix. The matrix helper now creates one manager (the account routes act on a manager) and saves one data source section (so the data source check never reaches a real service). The production code of `mod.rs` only registers routes.
2. **`src/cli.rs` لم يُمَس:** the step asked to make `generate_temp_password` and the email normalization `pub(crate)` "دون تغيير سلوكها", but both are already `pub` in `src/cli.rs` (`pub fn normalize_email`, `pub fn generate_temp_password`) and therefore already reachable from the dashboard module. Changing the visibility would have been a no-op or a regression, so the file is untouched (`git diff --numstat` = no entry).
3. **استعارة `devices_limiter` لحدود الكتابة:** the account routes (20/min) and the diagnostics (20/min) reuse the existing 20-per-minute limiter with their own keys (`admin:users:<id>`, `admin:diagnostics:<id>`), because `AppState` cannot gain a field (that would touch `src/main.rs` and the frozen `src/devices.rs`, which builds `AppState` in its tests). The device registration limit is untouched: the keys differ.
4. **ترتيب الترويسات (403/404) بعد تحليل الجسم:** axum parses the typed body before the handler runs, so a malformed body answers 400 before the role or the app check. The role check is still the first statement of every handler, and the tests send a valid body per route (`diagnostics_owner_only`, `diagnostics_unknown_app_is_404`). This is the same behaviour as the T10-A/T10-B routes.
5. **الدور عند فشل البحث:** in `identity/test`, a role lookup that fails leaves `role` empty (`null`) and the check still answers `ok:true` — the identity itself is already verified, and the role is extra information. The step does not say what happens when the data source is down during the role lookup.
6. **`claim_for_sending` يمنع الحظر:** the `NOT EXISTS` sub-query compares `b.app = campaigns.app` (the row being updated) — SQLite resolves the outer table name inside the sub-query, so the check is per row and not global.
7. **commit واحد أُعيدت كتابته:** the test `manager_gets_403_on_every_users_route` was strengthened (the manager now has a fresh re-authentication, so the refusal comes from the role and not from the missing re-auth) and the commit `T10C-2` was amended through a local interactive rebase, so the commit list of §6 stays exactly as prescribed. The branch was never pushed.
8. **`delete_user` يعيد `204` مع جسم JSON:** the handler answers `204 No Content` with a small JSON body for symmetry with the other write routes; the body is ignored by the client for 204.
9. **الحسابات المملوكة غير مذكورة في `GET /users`:** the list shows every account (owners included) with their role, as the step requires; the password hash is never in the row.

## 6. ملاحظات خارج النطاق

- (من T10-A/B) متابعة T13: حد `test-send` ما زال يشارك `auth_fail_limiter`، وحد الإرسال الحر على `topics_limiter`، واستخراج `build_router(state)` في `main()` ليُختبر end-to-end، واختبار مخصص لـ`dedupe.seconds`.
- (من T10-B) `PUT /apps/{app}/campaigns` هو مسار القسم و`GET /apps/{app}/campaigns` هو القائمة — يجب أن تعكس الواجهة (T12) ذلك.
- (من T9) مخبأ الأدوار يُبطَل عند كل حفظ قالب، والأنماط بلا `size_limit` ولا تثبيت، `validate_data_source` يقبل `http://localhost.evil.com`، `api_fallback` لا يغطي مسارَي الأجهزة، أسئلة `check.require`/`senders`/`claim_for_sending`.
- (من T2) تغطية ES256 غير مختبَرة، ورفض `alg=none` غير مغطّى، وشكل `tracing::error!(secret_env = ...)`.
- **إنشاء المالك ما زال بالـCLI فقط** كما يجب (T2)؛ لا يوجد مسار HTTP ينشئ مالكاً.
- مستودع `taqati`: لم يُمَس (لا commit ولا push).

## 7. أسئلة للمالك/المراجع

1. **`GET /users` يعرض المالكين أيضاً** (الدور والعapplications) — مقبول؟ (المطلوب: لا hash ولا سر.)
2. **حد `PUT /auth/test-uid` يشارك مفتاح `admin:users:<id>`** مع مسارات الحسابات: هل يُفضَّل مفتاح منفصل لهذا المسار (وهو لمسار واحد فقط)؟
3. **فشل البحث عن الدور في `identity/test` يعطي `role: null`** بدل 503 — موافق؟
4. **`DELETE /users/{id}` يعيد 204** — موافق على أن ينقص "الجلسات والتطبيقات تُحذف" تأكيداً في الرد؟
5. **`update_user` لا يقبل تغيير الدور** (لا يوجد حقل `role` أصلاً) ✓ — فقط للتوثيق.
6. **الخطوة التالية: T11** (مفاتيح التطبيقات: `src/auth.rs` و`src/admin/keys.rs`).

## 8. حالة git النهائية

```
$ git status --short
?? docs/plans/execution/T10C.md
(بعد هذا الـ commit يصبح كل شيء نظيفاً)

$ git log --oneline -7
<آخر commit>  docs(plan): T10C update execution tracker
5625489 feat(admin): T10C-3 add identity and data source test routes
7e52b24 feat(admin): T10C-2 add tool account routes and test uid
42293e7 fix(campaigns): T10C-1 refuse approval of a blocked entity atomically
1084dad docs(plan): T10B update execution tracker
```

- بلا `git push` (لم يُنفَّذ إطلاقاً). بلا `git reset --hard`/`git clean`/`git checkout -- .`/`git stash drop`. stash الموجود (`stash@{0}` بتاريخ 2026-09-23) لم يُنشأ ولم يُمَس.
- لا قيم أسرار في أي commit أو في هذا التقرير؛ الإشارة إلى الأسرار بالمسار فقط. (كلمة المرور المؤقتة في الاختبارات مولّدة لكل اختبار، ولا تُطبع في أي ردّ ولا في أي assertion.)

# تقرير T10-B

- **التاريخ/الوقت:** 2026-10-22، من 00:00 إلى 00:40 (توقيت الجهاز).
- **الفرع:** `feature/policy-auth` (تحقّق قبل البدء وقبل كل commit؛ لم يتغيّر).
- **HEAD قبل العمل:** `613d065` (`docs(plan): T10A update execution tracker`).
- **HEAD بعد العمل:** آخر commit هو `docs(plan): T10B update execution tracker` — 6 commits جديدة (5 كود + 1 توثيق)، بلا push.
- **ملف الخطوة المنفَّذ:** `docs/plans/execution/T10B.md` (الشريحة B من T10).

## 1. ما نُفِّذ (بند بند من ملف الخطوة)

| البند | الحالة | ملاحظة |
|---|---|---|
| §0 التعديلات المسموحة | تم | جديد: `src/admin/campaigns.rs`، `src/admin/history.rs`. تعديل: `src/admin/mod.rs` (تسجيل المسارات + `pub mod campaigns;`/`pub mod history;` + **تحديث اختبار المصفوفة** (انظر §5))، `src/db.rs` (قراءات وكتابات SQL للحملات والسجل وإحصاءات الأجهزة + `fail_stale_sending`)، `src/main.rs` (**استدعاء تعافي الإقلاع فقط**، 14 سطراً)، `src/campaigns.rs` (لم يُمَس: `dispatch_campaign` و`CampaignOutcome` كانا عامّين وكافيين)، `src/config/store.rs` (**إضافة `app_names()` فقط**، +34/−0). ممنوع: `src/identity|datasource|template.rs|send.rs|devices.rs|roles.rs|auth.rs|validation.rs|topics.rs|credentials.rs` وبقية `src/config` و`Cargo.toml` و`dashboard/` — لم يُمَس. |
| §1 `ConfigStore::app_names()` | تم | `src/config/store.rs`: `pub fn app_names(&self) -> Vec<String>` (مرتبة، من الذاكرة المؤقتة للمخزن). اختبار: `app_names_sorted_and_includes_configured`. |
| §1 `GET /apps` للمالك = الاتحاد | تم | `src/admin/apps.rs`: `list_apps` يجمع `state.config.app_names()` مع مفاتيح `project_map` (المالك) أو `user.apps` (المدير). اختبار: `list_apps_owner_includes_configured_without_credentials`. |
| §1 تعافي الإقلاع | تم | `db::fail_stale_sending(pool) -> Result<u64>`: كل حملة `sending` ⇒ `failed` مع `fcm_response='interrupted by restart'` و`sent_at = NULL`. تُستدعى في `main()` بعد فتح المخزن وقبل الاستماع، مع `tracing::warn!` إن كان العدد > 0، و`exit(1)` عند فشل الاستعلام. اختبارات: `stale_sending_campaigns_fail_on_startup`، `stale_recovery_leaves_other_statuses_alone`، `stale_recovery_runs_before_serving`. |
| §2 `GET /apps/{app}/campaigns` | تم | `limit` ≤ 100 (افتراضي 50)، `offset`، مُرشِّح `status` (مجهول ⇒ 400) و`entity_id`. كل عنصر: `id, entity_id, sender_uid, title, body, image, data, audience, condition, status, review_note, reviewer, created_at, decided_at, sent_at` و`fcm_response` مقتطعاً إلى 300 حرف. |
| §2 `POST .../approve` | تم | الترتيب: (1) الحملة تخص التطبيق وإلا 404 ⇒ (2) الكيان محظور ⇒ 409 `{"error":"entity_blocked"}` والحملة تبقى `pending_review` ⇒ (3) `claim_for_sending` (انتقال ذري يكتب `reviewer` و`decided_at` و`review_note` مع الحالة)؛ فشل ⇒ 409 `{"error":"not_pending"}` ⇒ (4) `dispatch_campaign` ⇒ 200 `{"status":"sent"|"failed"}` ⇒ (5) تدقيق `campaign.approve` بـ`id` و`entity_id` فقط. |
| §2 `POST .../reject` | تم | `{note}` إلزامي، 1..=200 حرفاً، بلا أحرف تحكم عدا `\n`. انتقال ذري `pending_review → rejected` مع `reviewer` و`decided_at` و`review_note`؛ فشل ⇒ 409. تدقيق `campaign.reject`. |
| §2 الحظر | تم | `GET /apps/{app}/blocks`، `POST /apps/{app}/blocks` (`{entity_id, reason?}`؛ `entity_id` بـ`[A-Za-z0-9_-]{1,64}`، `reason` ≤ 200) في **معاملة واحدة** (upsert في `campaign_blocks` + تحويل كل حملات الكيان `pending_review` إلى `cancelled` بـ`review_note='entity blocked'`) ويعيد `{"blocked":true,"cancelled":N}`، و`DELETE /apps/{app}/blocks/{entity_id}` (غير الموجود ⇒ 404). تدقيق `campaign.block`/`campaign.unblock`. |
| §2 المدير لتطبيقه فقط | تم | كل مسارات الحملات والحظر تستعمل `authorize(.., Need::AnyMember)`. اختبار: `campaign_routes_denied_to_unassigned_manager_with_403`. |
| §3 السجل | تم | `GET /apps/{app}/history?limit=&before=&kind=&status=` لهذا التطبيق فقط؛ `limit` ≤ 200 (افتراضي 50)، `before` تصفح بالمعرّف، `kind ∈ {raw,template,campaign,test}` وإلا 400. الحقول: `id, target, title, status, kind, ref, sender_uid, sender_email, created_at` و`response` مقتطعاً إلى 300. `GET /apps/{app}/history/{id}` يعيد `payload` أيضاً (وإلا 404). |
| §3 الإرسال الحر | تم | `POST /apps/{app}/send` بجسم `{title, body?, image?, data?, topic?|condition?|uid?}` بـ`deny_unknown_fields` و**هدف واحد بالضبط** (وإلا 400)؛ `uid` ⇒ `tokens_for_uid` ثم `send_to_tokens` وحذف الأجهزة الميتة، `topic`/`condition` ⇒ رسالة FCM واحدة مع حفظ الـtopic في جدول `topics` عند النجاح (كما يفعل `/send`). يمر بـ`validation::validate_send` (ولهدف `uid` وجهة token وهمية صالحة كما في T8). حد 30/دقيقة لكل مستخدم داشبورد ⇒ 429. سجل `kind="raw"`, `ref="dashboard"`, `sender_uid=<user.email>`. الرد `{"status":"success"|"failed","delivered","failed"}`، والفشل الكامل ⇒ 502. تدقيق `send.free` (الهدف فقط). **لا يقبل `token` خاماً أبداً** (الجسم لا حقل له). |
| §3 إحصاءات الأجهزة | تم | `GET /apps/{app}/devices/stats` ⇒ `{total, distinct_uids, by_platform:{android,ios,web}, active_last_7d, newest_last_seen}` من جدول `devices` لهذا التطبيق. **لا توكنات ولا uids.** دالة قراءة في `db.rs` (`device_stats`). |
| §4 الاختبارات (46 اسماً + 1) | تم | كل الأسماء المطلوبة موجودة وتعمل (فحص آلي بـ`cargo test -- --list`)، زيادةً على `init_unassigned_manager_is_403` الذي طلبه قرار المراجع على T10-A. العدد الكلي 497 (450 قبل + 47 جديد). |
| §5 الجودة | تم | `cargo build` بلا تحذيرات، `cargo clippy --all-targets` صفر تحذير، `cargo test` = 497 ناجح/0 فاشل/0 ignored، `rustfmt --check` نظيف على الملفات المعدّلة. |
| §5 التجارب السلبية الخمس | تم | كل تجربة أخفقت الاختبار المحدد ثم أُعيد الملف بنفس `sha256` (§3). |
| §6 الـ commits | تم | (1) `feat(config): T10B-1 add app_names and list all configured apps`، (2) `feat(admin): T10B-2 campaign list, approve and reject routes`، (3) `feat(admin): T10B-3 entity block routes`، (4) `feat(admin): T10B-4 history, free send and device stats routes`، (5) `fix(db): T10B-5 fail interrupted sending campaigns on startup`، (6) `docs(plan): T10B update execution tracker`. |

## 2. الملفات

`git diff --stat 613d065..HEAD -- src/`:

```
 src/admin/apps.rs      |   68 ++-
 src/admin/campaigns.rs | 1178 ++++++++++++++++++++++++++++++++++++++++++++
 src/admin/history.rs   |  872 ++++++++++++++++++++++++++++++++++
 src/admin/mod.rs       |  179 +++++++-
 src/config/store.rs    |   34 ++
 src/db.rs              |  398 +++++++++++++++-
 src/main.rs            |   14 +
 7 files changed, 2716 insertions(+), 27 deletions(-)
```

- **`src/admin/campaigns.rs` (جديد):** مراجعة الحملات: `list_campaigns`، `approve`، `reject`، `list_blocks`، `block_entity`، `unblock_entity` + نوع خطأ محلي `CampaignAdminError` (لأجسام 409 الموثّقة `{"error":"entity_blocked"}` / `{"error":"not_pending"}`) + 25 اختباراً.
- **`src/admin/history.rs` (جديد):** `list_history`، `history_detail`، `free_send`، `device_stats` + 15 اختباراً.
- **`src/admin/mod.rs` (تعديل):** `pub mod campaigns;` و`pub mod history;` وتسجيل 11 مساراً جديداً في `admin_routes()` (ومنها `PUT /apps/:app/campaigns` — §5)، وتحديث مصفوفة الصلاحيات لتغطي المسارات الجديدة (`matrix_covers_new_routes`).
- **`src/admin/apps.rs` (تعديل):** `list_apps` صار يعرض اتحاد التطبيقات المُعدّة والمُجهَّزة، ومعالج `put_campaigns_section` (§5)، واختبار `init_unassigned_manager_is_403`.
- **`src/db.rs` (تعديل):** `Campaign` صار يحمل `reviewer`/`decided_at`/`fcm_response`؛ `claim_for_sending` (توقيع جديد: المراجع والملاحظة، بلا `#[allow(dead_code)]`)، `reject_campaign`، `list_campaigns_filtered`، `list_blocks`، `block_entity` (معاملة واحدة)، `unblock_entity`، `read_history`، `read_history_detail`، `device_stats`، `fail_stale_sending` + 4 اختبارات جديدة.
- **`src/config/store.rs` (تعديل):** `app_names()` فقط (+34/−0) مع اختبارها.
- **`src/main.rs` (تعديل):** استدعاء `db::fail_stale_sending` عند الإقلاع فقط (14 سطراً).

## 3. الأوامر التي شغلتها ونتائجها

```
$ cargo build
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.77s      # بلا تحذيرات

$ cargo test
   test result: ok. 497 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 54.95s

$ cargo clippy --all-targets
    Checking fcm_microservice v0.1.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.93s      # صفر تحذير

$ rustfmt --check --edition 2021 --config skip_children=true \
      src/admin/mod.rs src/admin/apps.rs src/admin/campaigns.rs src/admin/history.rs \
      src/admin/auth.rs src/db.rs src/main.rs src/config/store.rs
   (بلا مخرجات)                                                            # نظيف

$ cargo test -- --list | grep -c ': test$'
497
```

**اختبارات الشريحة (47):** `admin::campaigns::tests` 25، `admin::history::tests` 15، `admin::tests::matrix_covers_new_routes` 1، `db::tests::stale_*` 3، `config::store::tests::app_names_sorted_and_includes_configured` 1، `admin::apps::tests::init_unassigned_manager_is_403` 1، `admin::apps::tests::list_apps_owner_includes_configured_without_credentials` 1.

**التجارب السلبية الخمس (§5) — نُفِّذت على الشيفرة النهائية، وأُعيد كل ملف بنفس البصمة:**

| # | التغيير المؤقت | الاختبار | المخرج الفعلي | `sha256` قبل/بعد |
|---|---|---|---|---|
| (أ) | `claim_for_sending` يقرأ الحالة ثم يحدّثها (بلا `WHERE status`) | `approve_twice_sends_once` | FAILED في **5 تشغيلات من 5**: `exactly one reviewer must win: [200, 200, 200, 200]` (يسار 4 / يمين 1) | `ddf3be9d2fa1b4ad1a26408972ceadbdd8d1f98de164355879ec3b02f1c7bcf1` (متطابق) |
| (ب) | الحظر لا يحوّل الحملات المعلقة | `block_cancels_pending_campaigns_in_one_transaction` | FAILED — `cancelled` = 0 بدل 2 | نفس البصمة (متطابق) |
| (ج) | `approve` لا يفحص الحظر | `approve_blocked_entity_is_409_and_stays_pending` | FAILED — 200 بدل 409 | `736e69d432f6525b2c3e879551fd149c763ecf7dcc186c7696bdc5c1d4a2a2cd` (متطابق) |
| (د) | الإرسال الحر يقبل حقل `token` خاماً | `free_send_rejects_raw_token_field` | FAILED — 200 بدل 400 | `82f45b1989b595e05c7f866905ea03277cf82ee2d1f21a6491d7d1ea2afc633a` (متطابق) |
| (هـ) | استدعاء `fail_stale_sending` محذوف من الإقلاع | `stale_recovery_runs_before_serving` | FAILED — `the startup must call the recovery` | `f734d7a8f2f279865e31e0d18a27b1b65f374b306b09621f79491cea1bdabf1e` (متطابق) |

**تجربة إضافية (معيار القبول 2 و3):** في T10-A أُثبت أن إضافة مسار بلا تغطية تُفشل اختبار المصفوفة. في T10-B تحقّقتُ من الأمر نفسه بمسار `/apps/:app/devices/stats` المضاف إلى `admin_routes()`: `matrix_owner_can_reach_every_route` و`matrix_covers_new_routes` يقارنان مجموعة المسارات المسجّلة بمجموعة حالات المصفوفة، فأي مسار جديد بلا حالة يُفشل الاختبارين. (لم أُضف مساراً وهمياً في T10-B لأن الآلية مُختبَرة في T10-A ومُثبتة في الكود.)

## 4. طريقة الفحص: كل بند من "طريقة الفحص" في الخطة/الخطوة ← النتيجة الفعلية وكيف تحققت

| البند | النتيجة | كيف تحققت |
|---|---|---|
| "اختبارات صلاحيات لكل مسار (owner/manager/غير مرتبط)" (خطة T10) | تم | 4 اختبارات صلاحيات جديدة + المصفوفة: `campaign_routes_denied_to_unassigned_manager_with_403` (6 مسارات ⇒ 403)، `campaign_routes_unauthenticated_401` (6 مسارات ⇒ 401)، `manager_can_approve_own_app_campaign` (200 + المراجع مسجّل)، `matrix_covers_new_routes` (كل مسار مسجّل مغطّى). |
| "`preview` لا يستدعي FCM" (خطة T10) | تم | لم يتغيّر مسار المعاينة؛ اختبار T10-A `preview_never_calls_fcm` ما زال ناجحاً ضمن الـ497. |
| "`test-send` بلا `test_uid` ⇐ 400" (خطة T10) | تم | لم يتغيّر؛ `test_send_without_test_uid_is_400` ما زال ناجحاً. |
| "حذف قالب مستخدم حديثاً ⇐ 409" (خطة T10) | تم | لم يتغيّر؛ `template_delete_recently_used_is_409` ما زال ناجحاً. |
| "كل تعديل يظهر في `audit_log`" (خطة T10) | تم | `audit_has_approve_reject_block_unblock_and_free_send_without_text`: الأفعال الخمسة موجودة، ولا نص إشعار ولا سبب حر طويل في التفاصيل. |
| §1 "الاتحاد في `GET /apps`" | تم | `list_apps_owner_includes_configured_without_credentials`: تطبيق مُعدّ بلا credentials يظهر للمالك بـ`configured=true` و`has_credentials=false`. |
| §1 "تعافي الإقلاع" | تم | `stale_sending_campaigns_fail_on_startup` (الحالة + `fcm_response` + `sent_at` فارغ)، `stale_recovery_leaves_other_statuses_alone` (5 حالات أخرى لم تتغيّر)، `stale_recovery_runs_before_serving` (فحص ترتيب المصدر في `main.rs`). |
| §2 "لا يُرسل شيء إلا بعد كسب الانتقال" | تم | `approve_twice_sends_once`: 4 مراجعين متزامنين ⇒ واحد 200 وثلاثة 409 وعدّاد FCM = 1. |
| §2 "الحظر يلغي الحملات المعلقة" | تم | `block_cancels_pending_campaigns_in_one_transaction`:campaignان معلّقان ⇒ `cancelled` + `review_note='entity blocked'`، والحملة `sent` وحملة كيان آخر لم تتغيّرا. |
| §2 "نسخة تخص تطبيقاً آخر ⇒ 404" | تم | `approve_foreign_app_campaign_is_404`. |
| §3 "لا توكنات ولا uids في الرد" | تم | `history_detail_never_contains_tokens`، `device_stats_counts_without_identifiers`، `free_send_uid_reaches_only_that_uids_devices` (توكيلات FCM belong to u1 فقط). |
| §5 "لا `unwrap/expect` خارج الاختبارات، لا `std::env`، كل SQL بـbind" | تم | مراجعة يدوية + `cargo clippy --all-targets` صفر تحذير؛ كل استعلامات SQL الجديدة تستعمل `?` للـbind؛ `std::env` غير موجود في الملفين الجديدين. |
| §5 "لا توكنات في `tracing`" | تم | رسائل `tracing` في هذا العمل: `warn!("{} interrupted campaign(s) marked as failed")` و`error!("Failed to recover the interrupted campaigns: {}", error)` — لا توكن ولا محتوى إشعار. |

## 5. انحرافات عن الأمر (وسببها)

1. **`src/admin/mod.rs` تجاوز "تسجيل المسارات فقط":**Apart from the route table and the two module declarations, the **test module** of `mod.rs` had to change, because the step itself requires the new routes to be covered by the permission matrix (`matrix_covers_new_routes`) and the shared test helper `login_request` became `pub(crate)` so the new modules can build a failing-FCM harness. The production code of `mod.rs` only registers routes.
2. **تعارض المسارات `GET` و`PUT` على `/apps/{app}/campaigns`:** the plan's table (3.11) has both the campaign list (`GET /apps/{app}/campaigns`) and the campaigns configuration section (`PUT /apps/{app}/campaigns`). Axum prefers the static segment, so the generic section route `PUT /apps/{app}/{section}` no longer matched `campaigns` and the dashboard could no longer save that section (405). Fix: the section save is registered a second time on the static path (`apps::put_campaigns_section`, a thin wrapper over `put_section` with the section name `campaigns`). The T10-A test `manager_cannot_change_campaign_senders_or_entity` (which failed with 405 in the middle of the work) is green again, and the new route is in the matrix.
3. **`blocked_entity_cannot_create_new_campaign` لا يستدعي مسار T9:** the whole API router is built inside `main()`, and `create_campaign` is private to the crate root, so the dashboard test harness cannot call the T9 HTTP route (main.rs may only be changed with the recovery call). The test asserts the **exact predicate** the T9 route uses before inserting — `db::is_entity_blocked(&pool, app, entity_id)` (in `src/main.rs`, the campaign creation step 7) — is true after the dashboard block, and the T9 route turns it into `403 Forbidden`. This is weaker than an end-to-end HTTP assertion; it is documented here on purpose.
4. **حد الإرسال الحر يعيد استخدام `state.topics_limiter`:** the step wants 30/minute per dashboard account with the key `admin:send:<user_id>`. `auth_fail_limiter` is 10/minute (too small) and adding a field to `AppState` would touch `src/main.rs` and the frozen `src/devices.rs` (which builds `AppState` in its tests). `topics_limiter` is an existing 30-per-minute limiter with a different key, so the IP limit of the topics routes is untouched.
5. **`stale_recovery_runs_before_serving` يفحص مصدر `main.rs`:** main.rs may only be modified with the recovery call, so the test cannot wrap the startup in a testable function. The test reads `src/main.rs` with `include_str!` and asserts that the call `db::fail_stale_sending` appears **after** `ConfigStore::new` and **before** `axum::serve`. This is a source-level order check, not a runtime one; the behaviour itself is covered by `stale_sending_campaigns_fail_on_startup`.
6. **الافتراضات غير المنصوص عليها:** the campaigns list defaults to `limit=50` (capped at 100) and the history list to `limit=50` (capped at 200) — the step gives only the caps. The approve body may carry an optional `note`; it is stored in `review_note` together with the atomic claim (the step does not say what the note does, and dropping it silently would be worse). The reject note is **not** copied into the audit (only `id` and `entity_id`), so the audit never holds a long free text. A free send to a uid without devices is a complete failure (`delivered == 0` ⇒ 502), like `/send`.
7. **أجسام 409 في وحدة جديدة:** `AdminError` lives in `mod.rs`, which may only be changed for the route registration, so `src/admin/campaigns.rs` defines its own error type to answer `{"error":"entity_blocked"}` and `{"error":"not_pending"}` exactly as the step requires.
8. **تعديل اختبار T9 `claim_for_sending_only_from_pending`:** the signature changed (`reviewer` and an optional note), so the test was updated — and **strengthened**: it now also asserts that `reviewer` and `decided_at` were written. The original assertions (first claim true, second claim false, a non-pending campaign false) are unchanged.
9. **commit إضافي:** the step lists 6 commits; the list was followed exactly (5 code + 1 docs). The extra work of T10-A (the route table) was not repeated.
10. **`src/campaigns.rs` لم يُمَس:** the step allowed changing it "only as needed" to use `dispatch_campaign` and its error map; `dispatch_campaign`, `CampaignOutcome` and `CampaignError` were already public, and the error map was written in the new dashboard module, so no change was needed.
11. **الخطوة لم تطلب `init_unassigned_manager_is_403`:** it was added because the reviewer's decision on T10-A required an existing test (or a new one) proving that an unassigned manager gets 403 before any disclosure.

## 6. ملاحظات خارج النطاق

- (من T10-A) حد `test-send` ما زال يشارك `auth_fail_limiter` — قرار المراجع: يُتابَع في T13.
- (من T10-A) لا يوجد اختبار مخصص لـ`dedupe.seconds` (القاعدة موجودة في `src/config/validate.rs`) — قرار المراجع: يُضاف في T13.
- (من T9) أسئلة مفتوحة: تجاوز `check.require` الذي يحوي `{sender.` في حالة مفتاح الخدمة، هل يفرض `GET`/`cancel` قيود `senders`، بقاء `#[allow(dead_code)]` على `claim_for_sending` (أُزيل في T10-B لأن المسار يستعمله الآن)، الحملات العالقة في `sending` (عولجت في T10-B عند الإقلاع).
- (من T9) مخبأ الأدوار يُبطَل عند كل حفظ قالب، والأنماط بلا `size_limit` ولا تثبيت، و`validate_data_source` يقبل `http://localhost.evil.com`، و`api_fallback` لا يغطي مسارَي الأجهزة.
- (من T2) تغطية ES256 غير مختبَرة، ورفض `alg=none` غير مغطّى، وشكل `tracing::error!(secret_env = ...)`.
- **الإرسال الحر في الداشبورد يرسل إلى أي topic للتطبيق** (وهو مقصود: الجدول 3.1 يسمح لمدير الداشبورد، والخطوة تذكره صراحة) — لكنه يرسل باسم شركة واحدة ومحتوى حر بلا مراجعة؛ المسار `owner` و`manager` معاً حسب الجدول 3.11.
- **سجل التدقيق يكتب `app` و`action` و`detail`:** تفاصيل `send.free` و`campaign.*` لا تحتوي نص الإشعار، لكن `detail` لإرسال حر.topic يكشف اسم الـtopic (وهو ليس سراً).
- مستودع `taqati`: لم يُمَس (لا commit ولا push).

## 7. أسئلة للمالك/المراجع

1. **تعارض `GET`/`PUT` على `/apps/{app}/campaigns`:** وُلد من جدول 3.11 نفسه (قائمة الحملات + قسم الإعداد). الحل الحالي تسجيل مزدوج للمسار الثابت. هل تُبقيه أم يُنقل مسار قائمة الحملات (مثلاً `/apps/{app}/campaign-list`)؟ (الأول يكسر واجهة API المخططة، والثاني يخالف الجدول.)
2. **`blocked_entity_cannot_create_new_campaign`:** مسارات T9 غير قابلة للاستدعاء من اختبارات الداشبورد (الموجّه كامل داخل `main()`). هل يُسمح باستخراج `api_routes` إلى دالة في `main.rs` ليصبح الفحص end-to-end؟
3. **حد الإرسال الحر على `topics_limiter`:** مقبول؟ (30/دقيقة، المفتاح `admin:send:<user_id>`.)
4. **`stale_recovery_runs_before_serving` فحص مصدر:** هل تفضّلون استخراج تهيئة الإقلاع إلى دالة قابلة للاختبار في `main.rs` (تعديل يتجاوز "استدعاء التعافي فقط")؟
5. **قائمة الحملات تعرض `condition` و`audience`:** مطلوب للوحة، وليس سراً. هل يُبقى؟
6. **`review_note` عند الموافقة:** الخطوة تذكر `{note?}` دون تحديد وظيفتها؛ خُزّنت كملاحظة المراجعة. موافق؟
7. **الشريحة C (حسابات الأداة + `identity/test` + `data_source/test`)** هي المتبقية من T10.

## 8. حالة git النهائية

```
$ git status --short
?? docs/plans/execution/T10B.md
(بعد هذا الـ commit يصبح كل شيء نظيفاً)

$ git log --oneline -7
<آخر commit>  docs(plan): T10B update execution tracker
2f65f3f fix(db): T10B-5 fail interrupted sending campaigns on startup
cf03e69 feat(admin): T10B-4 history, free send and device stats routes
0ca8f9c feat(admin): T10B-3 entity block routes
fb9492c feat(admin): T10B-2 campaign list, approve and reject routes
7bb3b1e feat(config): T10B-1 add app_names and list all configured apps
613d065 docs(plan): T10A update execution tracker
```

- بلا `git push` (لم يُنفَّذ إطلاقاً). بلا `git reset --hard`/`git clean`/`git checkout -- .`/`git stash drop`. stash الموجود (`stash@{0}` بتاريخ 2026-09-23) لم يُنشأ ولم يُمَس.
- لا قيم أسرار في أي commit أو في هذا التقرير؛ الإشارة إلى الأسرار بالمسار فقط.

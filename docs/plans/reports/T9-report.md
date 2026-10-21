# تقرير T9

- التاريخ/الوقت: 2026-10-21 (توقيت الجلسة المحلية)
- الفرع: `feature/policy-auth`
- HEAD قبل T9: `5ecb57c` — HEAD بعد كود T9: `f4343ca` (قبل commit الوثائق)
- الـ commits: `9afce24` (T9-1) ← `ffe4334` (T9-2) ← `4b4be31` (T9-3) ← `f4343ca` (T9-4) ← ثم commit الوثائق (T9-5)

## 1. ما نُفِّذ (بند بند من ملف الخطوة)

| البند | الحالة | ملاحظة |
|---|---|---|
| §1 `template.rs`: `Snippet`/`SnippetMode`/`render_snippet` | تم | `Params` داخلي جديد (`Validated`/`Plain`) ليستعمل المساران المنطق نفسه؛ `template: Option<&Template>` في `Renderer` (بلا `vars` في الـsnippet) |
| §1 `validate_param_map` بنفس منطق `validate_params` | تم | استُخرج `validate_param_map_inner` واستُعمل من الدالتين (لا نسخة) |
| §1 `check_require_map` بسلوك `require` نفسه (403 موحد) | تم | يعيد `Forbidden` للوثيقة الغائبة/الحقل الغائب/عدم التطابق |
| §2 جداول `campaigns` و`campaign_blocks` + فهرسان بصيغة 3.14 حرفياً | تم | في `migrate` بعد `template_dedupe`، idempotent |
| §2 `insert_campaign_checked` داخل معاملة واحدة وتحت قفل كتابة | تم | `CAMPAIGN_WRITE_LOCK` + `tx` واحدة؛ النوافذ 24h/7d على `pending_review|sending|sent` بحساب `strftime('%s', created_at)` |
| §2 `InsertError::QuotaExceeded{retry_after_seconds}` (الأقدم+النافذة−الآن، ≥1) | تم | اليوم أولاً ثم الأسبوع؛ `.max(1)` |
| §2 قاعدة `review.mode` (`none`/`always`/`first_n`) + تخطي service | تم | `first_n` يعدّ `sent|sending` فقط؛ `pending_review` لا يُحتسب |
| §2 `get_campaign`, `list_campaigns(limit≤100)`, `claim_for_sending`, `cancel_pending`, `finish_campaign`, `is_entity_blocked` | تم | كل SQL بـbind؛ `limit.min(100)`؛ `finish_campaign` يقتطع إلى 2000 حرفاً ويضع `sent_at` عند `sent` فقط |
| §3.1 `build_condition` + القيود والتفريد والعدد | تم | حدود الحجم قبل أي عمل؛ `required_topic` يُتجاهل بصمت؛ التكرار داخل المجموعة يُزال؛ العدد المختلف + required ≤ `max_topics` |
| §3.1 النمط الحرفي/`*`/`{p.<param>}` | تم | `*` يلزمه لاحقة `[A-Za-z0-9_]+` غير فارغة؛ placeholder يُستبدل فقط إذا `entity_id` صالح؛ أي topic آخر ⇒ `topic not allowed` |
| §3.1 الفحص النهائي (الطول + `validation.rs` + محارف topic) | تم | `validate_topic_name` لكل topic، ثم طول الشرط ثم `validate_condition` |
| §3.2 title/body والحدود وأحرف التحكم | تم | `title_max`/`body_max` بالأحرف؛ العنوان المرسل غير فارغ بعد `trim`؛ `\n` مسموح في body فقط |
| §3.2 العنوان النهائي = `render_snippet(title_prefix)` + title ثم `validation.rs` | تم | عبر `validate_send` في المسار |
| §3.2 كشف الروابط (best-effort) | تم | تعبير واحد غير حساس للحالة مع كل الأنماط المطلوبة + `@\S+\.\S+`، وتعليق صريح أنه ليس حدوداً أمنية |
| §3.2 الصورة: https + مضيف تمام + رفض `@`/`\`/مسافة/`:`/أحرف كبيرة/نطاق فرعي + الطول | تم | عبر `config::validate::is_image_host` + قائمة `image_hosts`؛ القائمة الفارغة ترفض كل صورة |
| §3.2 الإجراء: `action` من `content.actions` + `action_params` + `data` + `check` | تم | `validate_param_map`؛ `data` تُصيَّر من الإعداد فقط؛ `check.doc` عبر مصدر البيانات ثم `check_require_map` ⇒ 403 موحد؛ `Unavailable` ⇒ 503 |
| §3.3 ترتيب `POST /campaigns` 1..12 | تم | الكود معلَّق بالأرقام: app ⇒ IP ⇒ (service أو هوية+دور) ⇒ enabled ⇒ senders/uid ⇒ entity_id ⇒ وثيقة+require ⇒ حظر ⇒ محتوى ⇒ جمهور ⇒ validate_send ⇒ إدراج ⇒ dispatch ⇒ 201 |
| §3.3 `GET /campaigns` (1–3 و6، بلا fcm_response/condition) | تم | `entity_id` إلزامي لغير service؛ service بلا `entity_id` ⇒ كل التطبيق؛ ترتيب `id DESC` |
| §3.3 `POST /campaigns/{id}/cancel` | تم | حملة تطبيق آخر ⇒ 404؛ الملكية بنفس خطوة 6؛ غير مملوكة ⇒ 404 (قرار)؛ غير `pending_review` ⇒ 409 `Campaign is not pending review` |
| §3.4 `dispatch_campaign` | تم | يقرأ بشرط `sending`، `send::build_message(Destination::Condition)`، `finish_campaign`، سجل `kind="campaign"`, `ref=<id>`, `target="condition"`، بلا توكنات |
| §4 كل أسماء الاختبارات (58 اسماً) | تم | 21 في `campaigns.rs` + 10 في `db.rs` + 27 في `main.rs`، بلا `ignore` |
| §5 الجودة والتجارب الخمس | تم | انظر §3 و§4 |
| §6 الـ commits | تم | 4 commits كود + commit الوثائق |

## 2. الملفات (`git diff --stat 5ecb57c..HEAD`، قبل commit الوثائق)

```
 src/campaigns.rs | 1072 +++++++++++++++++++++++++++++++++++++++++++++++
 src/db.rs        |  706 +++++++++++++++++++++++++++++++++++++++-
 src/main.rs      | 1488 +++++++++++++++++++++++++++++++++++++++++++++++++++++-
 src/template.rs  |  120 ++++-
 4 files changed, 3372 insertions(+), 14 deletions(-)
```

- `src/campaigns.rs` (جديد): `CampaignError`، `build_condition`، `prepare_content`، `resolve_entity`، `check_action`، `dispatch_campaign`، `insert_campaign`، و21 اختباراً.
- `docs/plans/reports/T9-report.md` (جديد، هذا الملف).
- `docs/plans/execution/T9.md`: كان غير متتبعاً في الشجرة، أُضيف في commit الوثائق.
- `src/template.rs`: إضافة `Snippet`/`SnippetMode`/`render_snippet`/`validate_param_map`/`check_require_map` + إعادة هيكلة داخلية (`Params`, `Option<&Template>`). **لم تُعدَّل أي اختبار T7** (الفرق ينتهي قبل كتلة `#[cfg(test)]`).
- `src/db.rs`: الجداول + 7 دوال + `NewCampaign`/`Inserted`/`InsertError` + 10 اختبارات.
- `src/main.rs`: `mod campaigns;` + 3 مسارات + `CampaignHttpError` + مساعدات + 27 اختبار HTTP. `AppState` لم يتغير.

## 3. الأوامر التي شغلتها ونتائجها

| الأمر | النتيجة |
|---|---|
| `git rev-parse --abbrev-ref HEAD` قبل كل commit | `feature/policy-auth` في الأربعة |
| `cargo build` بعد T9-1 | نجاح مع تحذيرات `dead_code` مؤقتة (الواجهات لم تُستعمل بعد) |
| `cargo test db::` بعد T9-2 | 17 ناجحاً، 0 فاشل |
| `cargo test campaigns::` بعد T9-3 | 21 ناجحاً، 0 فاشل |
| `cargo test` (كامل، قبل إضافة اسم المحتوى الناقص) | 393 ناجحاً، 0 فاشل |
| `cargo test` (كامل، نهائي) | **394 ناجح، 0 فاشل، 0 متجاهل** (23.28s) |
| `cargo test -- --list` | `394 tests, 0 benchmarks` |
| `cargo build` (نهائي) | `Finished` بلا أي تحذير |
| `cargo clippy --all-targets` (نهائي) | 0 تحذير / 0 خطأ |
| `rustfmt --check --config skip_children=true --edition 2021` على الملفات الأربعة | `fmt ok` |
| فحص الأسماء الثابتة في `--list` | 58/58 موجودة (بعد إضافة `action_check_foreign_product_is_forbidden` وإعادة تسمية/توزيع الاختبارات) |
| فحص العربية في `src/` | الجديد فقط في بيانات اختبار: `src/campaigns.rs:925-926,932-933` (العنوان والنص العربي)؛ الباقي سابق (مثل `src/main.rs:2332-2336`، `src/template.rs:802-805`) |
| فحص `unwrap(`/`expect(`/`std::env` خارج الاختبارات | لا شيء في `campaigns.rs`، ولا في دوال T9 في `db.rs`، ولا في مسارات الحملات في `main.rs` |
| `sha256sum` قبل/بعد كل تجربة سلبية | مطابق بعد الاستعادة (انظر §4) |

### التجارب السلبية الخمس (أُعيدت ثم أُرجعت النسخة الأصلية)

| التجربة | ما فعلته | النتيجة الفعلية |
|---|---|---|
| (أ) مطابقة مضيف الصورة بلاحقة | استبدلت `allowed == host` بـ`host.ends_with(allowed)` وحذفت `is_image_host` | فشل `image_rejects_userinfo_port_backslash_and_subdomain` عند السطر 966 (النطاق الفرعي قُبل) |
| (ب) `first_n` يعدّ `pending_review` | أضفت `'pending_review'` إلى استعلام العدّ | فشل `first_n_review_counts_sent_and_sending_only`: `left: "sending" right: "pending_review"` |
| (ج) نقل فحص الحصة خارج المعاملة | حذفت القفل والبداية، وجعلت استعلامَي العدّ على `pool` ثم بدأت المعاملة قبل الإدراج | فشل `quota_insert_is_race_safe` **5/5 تشغيلات** (`FAILED` كل مرة)، بلا حاجة لتقوية الاختبار |
| (د) تخطي `check_require_map` لإجراء المنتج | `if prepared.check.is_some() { return Ok(()); }` في بداية `check_action` | فشل `action_check_foreign_product_is_forbidden`: `left: 201 right: 403` (منتج شركة أخرى قُبل) |
| (هـ) لا تحقق من الكيان في `cancel` | حذفت كتلة `resolve_entity` من مسار الإلغاء | فشل `cancel_other_entity_is_404`: `left: 200 right: 404` |

استُرجعت الملفات من نسخ `/tmp/t9exp` وتحقّق `sha256sum -c`: `campaigns.rs` = `22f5bca5…`، `db.rs` = `20ea2df9…`، `campaigns.rs` = `1188f007…`، `main.rs` = `3014b107…`، ثم أُعيد تشغيل الاختبار المستهدف ونجح في كل مرة.

## 4. طريقة الفحص (من الخطة/الخطوة ← النتيجة وكيف تحققت)

| الفحص المطلوب | الاختبار | النتيجة |
|---|---|---|
| الشرط الحرفي من 3.1 | `condition_matches_expected_text_literally` | `assert_eq!` على النص `'promos' in topics && ('country_iq' in topics) && ('technicals' in topics || 'users' in topics) && ('lang_ar' in topics)` |
| التفريد والترتيب | `condition_dedups_and_orders` | `users` مكرر مرة واحدة، والترتيب محفوظ |
| topic خارج القائمة | `condition_rejects_topic_outside_allowed` | 400 `topic not allowed` |
| `admins`/`all` | `condition_rejects_admins_and_privileged_topics` | 400 بالدورتين |
| 5 topics + promos | `condition_enforces_max_topics_including_required` و`five_topics_plus_promos_is_400` | 400 |
| `*` بلاحقة فارغة | `condition_pattern_star_requires_non_empty_suffix` | `news_` ⇒ 400، `news_x` ⇒ 200 |
| placeholder الكيان | `condition_entity_placeholder_topic_matches_only_own_entity` / `condition_rejects_bad_entity_id_for_placeholder_topic` | `company_acme` فقط، ومعرّف 65 حرفاً لا يطابق |
| مجموعات فارغة/كثيرة | `condition_empty_groups_and_oversized_groups_rejected` | 400 (`groups` فارغة، 6 مجموعات، 11 topic) |
| عدم قبول إدخال في الشرط | `condition_never_contains_unquoted_input` | كلاهما 400، والناتج الصالح مقيّد بالاقتباس |
| prefix من الوثيقة | `title_prefix_comes_from_entity_name` | `Acme: Sale` و`company_id` في data |
| حدود العنوان/النص | `title_and_body_limits_enforced` | 11 عنواناً، 6 نصاً، عنوان فراغ ⇒ رفض؛ الحدود الدقيقة تمر |
| الروابط | `links_rejected_in_title_and_body` / `links_rejected_case_insensitive_and_dotted_domains` / `link_in_body_is_400` | `://`، `www.`، `t.me/`، `wa.me/`، `bit.ly`، `@x.y`، `EXAMPLE.COM`، `sub.domain.biz` ⇒ رفض |
| نص عربي بلا روابط | `arabic_text_without_links_accepted` | مقبول |
| الصورة | `image_exact_host_only` / `image_rejects_userinfo_port_backslash_and_subdomain` / `image_rejected_when_no_hosts_configured` / `external_image_is_400` | المضيف التام فقط؛ `@`/`:443`/`\`/نطاق فرعي/قائمة فارغة ⇒ رفض |
| الإجراء | `action_must_be_configured` / `action_params_validated` / `action_data_rendered_from_config_only` | إجراء مجهول ومعامل مجهول/ناقص ⇒ 400؛ قيمة عميل بشكل placeholder تبقى حرفية |
| منتج شركة أخرى | `action_check_foreign_product_is_forbidden` + `foreign_product_action_is_403` | منتج مملوك ⇒ 201 `sent`، منتج شركة ثانية ⇒ 403 بلا إرسال |
| شركة معلقة/غير مملوكة/محظورة | `pending_company_is_forbidden` / `other_companys_entity_is_forbidden` / `blocked_entity_is_forbidden` / `forbidden_responses_are_uniform` | 403 و`Forbidden` متطابقة |
| رفض قبل كشف الكيان | `campaign_role_not_in_senders_is_403`، `campaign_without_token_is_401` | 403/401 |
| مفتاح قديم/تطبيق مجهول/معطّل | `legacy_key_cannot_create_campaign` / `campaign_unknown_app_is_404` / `campaigns_disabled_is_404` | 401/404/404 `Campaigns are not enabled` |
| الحصة اليومية | `second_campaign_same_day_is_429_with_retry_after` + `quota_day_and_week_enforced` | 429 `{"error":"quota_exceeded","retry_after":N}` (N≥1)، ونافذة اليوم/الأسبوع بالثواني |
| الحالات غير المحسوبة | `quota_ignores_rejected_cancelled_failed` | لا تستهلك حصة |
| سباق الحصة | `quota_insert_is_race_safe` (10 مهام متزامنة) | 3 نجاحات بالضبط لسقف 3، و3 صفوف في القاعدة |
| `first_n` | `first_n_review_counts_sent_and_sending_only` | `pending_review` لا يُحتسب؛ `sent`+`sending` يُحتسبان |
| `always`/`none`/service | `review_always_and_none` | `pending_review` / `sending` / `sending` |
| أول 3 ثم الرابعة | `first_three_campaigns_pending_review_fourth_sent` | أول 3 `pending_review`؛ بعد تحويلها إلى `sent` (محاكاة موافقة T10) الرابعة `sent` ورسالة FCM واحدة |
| الشرط الفعلي إلى FCM | `sent_campaign_calls_fcm_with_exact_condition` | `message.condition` = النص الحرفي؛ لا `token`/`topic` |
| فشل FCM | `failed_fcm_marks_campaign_failed` | 201 `failed`، صف `failed`، سجل `failed` |
| السجل | `campaign_history_row_has_kind_and_ref` | `kind=campaign`، `ref=id`، `target=condition`، `sender_uid`، `success` |
| الإلغاء | `cancel_pending_ok_and_sent_is_409` / `cancel_other_entity_is_404` / `cancel_pending_only_from_pending` | 200 ثم 409، وغير المملوكة 404 (نفس رسالة غير الموجودة) |
| القائمة | `list_returns_only_own_campaigns_without_internals` / `list_campaigns_scoped_by_entity_and_limit_capped` | حملات الكيان فقط، بلا `condition`/`fcm_response`، السقف 100، `offset` يعمل |
| انقطاع المصدر | `campaign_source_unavailable_is_503` | 503 `the data source is unavailable` |
| JWT + مصدر HTTP | `campaign_works_with_jwt_identity_and_http_source` | 201 `sent` والعنوان `Acme: Weekend sale` |
| الحقن | `injection_in_audience_topic_cannot_alter_condition` | 400 للقيمتين، 0 رسالة إلى `FakeTransport` |
| الحقن في العنوان | `injection_in_title_cannot_alter_condition` | 201 ثم الشرط المخزَّن مطابق حرفياً و0 إرسال (انظر §5) |

## 5. انحرافات عن الأمر (وسببها)

1. **`injection_in_title_cannot_alter_condition` يتوقع 201 لا 400.** الخطوة تقول "⇒ 400 دائماً" لكل قيم الحقن، لكن قيمة العنوان لا تدخل تركيب الشرط إطلاقاً: الشرط يُبنى من `audience.groups` فقط، والعنوان يمرّ عبر `validate_send` الذي يفحص الطول لا محارف الاقتباس. رفض `'` في العنوان كان سيكسر نصاً مشروعاً (مثل "Acme's sale") بلا فائدة أمنية. الاختبار يثبت الخاصية الأمنية المطلوبة: الحالة 201، الشرط المخزَّن `'promos' in topics && ('users' in topics)` حرفياً، و0 رسالة إلى `FakeTransport` (المراجعة `always`). أما الحقن في topic الجمهور فيرد 400 كما هو مطلوب. أُدرج هذا أيضاً في §7.
2. **`first_three_campaigns_pending_review_fourth_sent` يحوّل الصفوف الثلاثة إلى `sent` بـSQL** بعد إنشائها `pending_review`، لأن T9 لا تحتوي مسار موافقة (T10). قاعدة `first_n` نفسها مُختبرة في `db.rs` بلا هذا التحويل، والاختبار يثبت أن الرابعة تُرسل بعد استيفاء n.
3. **`action_check_foreign_product_is_forbidden` اختبار HTTP لا اختبار وحدة.** كتبته في `main.rs` لأنه يمرّ عبر `check_action` الحقيقي (قراءة `store_products/{p.product_id}` من مصدر HTTP) ويثبت أيضاً أن المنتج المملوك يمرّ (201) والغريب 403؛ اختبار وحدة لـ`check_require_map` مباشرةً لن يكشف تخطي `check_action` (وهو ما تطلبه التجربة (د)).
4. **`#[allow(dead_code)]` على `claim_for_sending`.** الدالة مطلوبة في §2 ومُستعملة في اختبار `claim_for_sending_only_from_pending` لكن لا مسار T9 يستدعيها (الموافقة في T10)؛ بدون السماح يظهر تحذير `dead_code` في `cargo build`. أُبقيت مع تعليق يذكر أن T10 هي المستهلك، بدل حذف واجهة نصّت الخطوة عليها.
5. **حقل `fcm_response` غير موجود في `struct Campaign`.** الخطوة تطلب إقصاءه من القائمة، ولا يقرأه شيء في T9؛ إبقاؤه كان يولّد تحذير `dead_code`. `finish_campaign` ما زال يكتبه في القاعدة، وT10 تضيفه للقراءة عند الحاجة.
6. **`review` تُمرَّر داخل `NewCampaign` وليس ضمن `limits`.** الخطوة تحدد توقيع `insert_campaign_checked(pool, new, limits, now)` بلا وسيط للمراجعة؛ أضفت `review: Review` و`sender_is_service: bool` إلى `NewCampaign` (كما كان مقرراً في سياق T9) بدل تغيير التوقيع.
7. **ترتيب فحص الحصة: اليوم قبل الأسبوع.** عند تجاوز الاثنين يُعاد `retry_after` الأقصر (اليوم)، وهو الأنفع للعميل؛ الخطوة لا تحدد الترتيب.
8. **`GET` لا يفحص `campaigns.senders`.** الخطوة تنص على "نفس خطوات 1–3 و6" فقط، وخطوة 4 (senders/uid) ليست ضمنها؛ الأمان من ملكية الكيان في خطوة 6. `POST` و`cancel` كذلك لا يفحصان senders في `cancel` (الخطوة تذكر 1–3 و6)، لكن `POST` يفحصها كما هو مطلوب.

## 6. ملاحظات خارج النطاق (لم تُصلح)

- فشل `dispatch_campaign` لأسباب داخلية (قاعدة/`project_map`/إرسال) يترك الحملة بحالة `sending` بلا مسار استرجاع؛ لا إعادة محاولة في T9. يحتاج قراراً في T10/T11.
- مفتاح الخدمة لا يتخطى `check.require` للإجراء (فقط `entity.require` يُفلتر حسب قيمة `{sender.`). حملة service بإجراء فيه `owner_id: "{sender.uid}"` سترد 403 دائماً. لم أوسّع التخطي لأنه غير مذكور (انظر §7).
- `insert_campaign_checked` يعتمد على `created_at` الذي تكتبه SQLite (`CURRENT_TIMESTAMP`، UTC) بينما `now` يأتي من `SystemTime`؛ فرق الثواني الضئيل لا يؤثر على النوافذ، لكن الاختبارات استخدمت تسامحاً ±2 ثانية حيث تقارن `retry_after`.
- ملاحظات T8-F المفتوحة باقية كما هي (مخبأ الأدوار يُبطل عند كل حفظ قالب، `validate_template` يسمح بـ`{var}` في `require`، إلخ)؛ لم أمسّها.
- `docs/plans/secure-sending-plan.md` قسم T9 "الحالة" ما زال ⬜ في نسخة الخطة الحالية (كذلك T8)؛ المنفذ يحدّث سجل التقدم فقط.

## 7. أسئلة للمالك/المراجع

1. هل تُبقي على قرار اختبار الحقن في العنوان (201 مع إثبات ثبات الشرط) بدل 400؟ إن لزم 400 حرفياً فأقرب تفسير هو رفض `'`/`||` في العنوان، وهو ما لم أنفذه لأنه يكسر نصاً مشروعاً.
2. هل يجب أن يتخطى مفتاح الخدمة أيضاً `check.require` للإجراءات (مثل `open_product`) تماماً كما يتخطى حقول `entity.require` التي تحتوي `{sender.`؟
3. هل يجب أن يفحص `GET`/`cancel` عضوية الدور في `campaigns.senders` (خطوة 4) إضافة إلى ملكية الكيان؟ طُبّق حرفياً: لا.
4. `claim_for_sending` بلا مستهلك في T9؛ أتبقى `#[allow(dead_code)]` حتى T10 أم تُحذف وتُضاف عند الحاجة؟
5. ماذا يحدث لحملة فشل إرسالها الداخلي وبقيت `sending`؟ هل تُعالَج في T10 (إعادة محاولة/تعليم `failed`) أم تُترك؟
6. هل يؤكد المراجع أن `first_n` يعني "أول n تُراجع" مع كون `pending_review` لا يُحتسب؟ (هذا ما نصّت عليه الخطوة وطُبّق واختُبر.)

## 8. حالة git النهائية

```
$ git status --short        # بعد commit الوثائق
(نظيف)

$ git log --oneline -5
<هذا الـcommit>  docs(plan): T9 update execution tracker
f4343ca feat(campaigns): T9-4 add create, list and cancel routes
4b4be31 feat(campaigns): T9-3 audience condition and content rules
ffe4334 feat(db): T9-2 add campaigns tables, quota and review rules
9afce24 feat(template): T9-1 expose snippet rendering helpers

$ git rev-parse --abbrev-ref HEAD
feature/policy-auth
```

- لم يُدفع أي شيء. لم تُعدَّل `taqati`. لا أسرار في أي commit أو مخرج.
- كل commit يحمل `Refs: docs/plans/secure-sending-plan.md T9`.

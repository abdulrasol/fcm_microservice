# تقرير T8-F

- التاريخ/الوقت: 2026-10-21
- الفرع: `feature/policy-auth`
- HEAD قبل T8-F: `f3d94fa` — HEAD بعد كود الإصلاح: `0a181e2` (وهذا التقرير داخل commit الوثائق التالي)

## 1. ما نُفِّذ (بند بند من ملف الخطوة)

| البند | الحالة | ملاحظة |
|---|---|---|
| تمرير كل خروج بخطأ بعد الخطوة 9 عبر تحرير المفتاح | تم | دالة داخلية `deliver(...) -> Result<Delivery, (StatusCode,String)>` تحتوي الخطوتين 10–11، والمستدعي يحرّر المفتاح في ذراع `Err` فقط |
| «الأبسط دالة داخلية» كما نصّت الخطوة | تم | `deliver` تأخذ `&AppState` و`app` و`&AppSnapshot` و`&Rendered` وتعيد `Delivery {target, delivered, failed, dead_tokens, empty_audience}` |
| عدم تكرار `release_dedupe` يدوياً في كل فرع | تم | مساعد واحد `release_dedupe_key(...)`، يُستدعى في موضعين فقط: ذراع `Err`، وفرع `delivered == 0` بعد النجاح |
| فشل التحرير لا يخفي الخطأ الأصلي | تم | داخله `let _ = db::release_dedupe(...)`، والخطأ الأصلي يُعاد كما هو |
| بقاء الردود/الرموز حرفياً | تم | 330 اختبار T8 السابقة تمر دون أي تعديل، والاختبارات الخمسة الجديدة تؤكد `no_devices`/`success`/`duplicate` كما كانت |
| حذف `#![allow(dead_code)]` من `template.rs` | تم | لم يظهر أي تحذير بعد الحذف، فلم يُعد أي `#[allow]` على أي عنصر |
| مسح `#[allow(dead_code)]` من `devices.rs` حيث صار مستعملاً | تم | أُزيلت السبعة كلها (`tokens_for_uid`, `tokens_for_uids`, `remove_dead_token`, `tokens_for_role`, `RoleDirectory`, `Default`, `impl RoleDirectory`) بعد أن أثبت `cargo build` أنها مستعملة |
| `dedupe_released_when_device_lookup_fails` | تم | إسقاط جدول `devices` ⇒ 500 `device storage failed` (لا 503، انظر §5) ثم `migrate` وإعادة الطلب ⇒ `no_devices` وليس `duplicate` |
| `dedupe_released_when_role_source_unavailable` | تم | مصدر `http://127.0.0.1:1` ⇒ 503، ثم مصدر محلي عامل + جهاز ⇒ `success` |
| `dedupe_released_when_role_members_missing` | تم | 400 `role targets require identity.role_members`، ثم ضبط `role_members` ومصدر عامل ⇒ `success` |
| `dedupe_released_when_credentials_missing` | تم | حذف `project_map` ⇒ 500 `Service account for this app is not loaded`، ثم إعادته ⇒ `success` |
| `dedupe_still_blocks_after_successful_delivery` | تم | نجاح أول ⇒ الثاني `duplicate` وعدد رسائل FCM 1 |
| التجربة السلبية الملزمة | تم | حجبت التحرير لهدف `uid` فقط ⇒ فشل الاختبار المطلوب، ثم أُعيد الملف وتحقّق `sha256` |
| `cargo fmt` / `build` / `clippy --all-targets` صفر تحذير / `cargo test` كامل | تم | 335 ناجح، 0 فاشل، 0 متجاهل؛ clippy `Finished` بلا تحذير |

## 2. الملفات (`git diff --stat f3d94fa..0a181e2`)

```
 src/devices.rs  |   7 --
 src/main.rs     | 344 ++++++++++++++++++++++++++++++++++++++++++++++++++-----------
 src/template.rs |   1 -
 3 files changed, 281 insertions(+), 71 deletions(-)
```

- `src/main.rs`: هيكلة `send_template` بعد الخطوة 9 + `Delivery` + `release_dedupe_key` + `deliver` + 5 اختبارات HTTP. (`AppSnapshot`/`Rendered` استيرادان جديدان).
- `src/template.rs`: حذف السطر 12 `#![allow(dead_code)]` فقط.
- `src/devices.rs`: حذف سبعة أسطر `#[allow(dead_code)]` وتعليقاتها فقط، بلا لمس منطق.
- `docs/plans/execution/T8-fix.md` (جديد، أُضيف في commit الوثائق)، `docs/plans/reports/T8-fix-report.md` (هذا الملف)، `docs/plans/execution/README.md`، `docs/plans/secure-sending-plan.md`.

## 3. الأوامر التي شغلتها ونتائجها

| الأمر | النتيجة |
|---|---|
| `cargo build` بعد الهيكلة وقبل التنظيف | نجاح بلا تحذير (لا dead_code، فلم يُعد أي allow جزئي) |
| `cargo build` بعد حذف `#![allow(dead_code)]` وسبعة allows | نجاح بلا تحذير |
| `cargo test dedupe_` | 10 ناجحة (الخمسة الجديدة + خمسة قديمة بالاسم نفسه) |
| `cargo clippy --all-targets` | `Finished` بلا أي تحذير |
| `cargo test` كامل | **335 ناجح، 0 فاشل، 0 متجاهل** (41.16s) |
| فحص `#![allow(dead_code)]` في `template.rs` | غير موجود |

التجربة السلبية:
- قبل: `sha256(src/main.rs) = 21753c9c93e0fe2406e0d9fc07ee99f23859452c37922e5a16d5eae60b06c62b`
- التعديل المؤقت: في ذراع `Err` صار التحرير يُنفَّذ فقط عندما لا يكون الهدف `ResolvedTarget::Uid(_)` (أي أُزيل من مسار `tokens_for_uid`).
- النتيجة: فشل `dedupe_released_when_device_lookup_fails`:
  ```
  left: {"delivered":0,"failed":0,"status":"duplicate"}
  right: {"delivered":0,"failed":0,"status":"no_devices"}
  ```
- بعد إعادة النسخة: `sha256sum -c` أعطى `src/main.rs: OK` (البصمة نفسها)، والاختبار عاد ناجحاً.

## 4. طريقة الفحص (معايير القبول الثلاثة)

| المعيار | النتيجة وكيف تحققت |
|---|---|
| 1. الفرق ضمن الملفات المسموحة، والردود/الرموز لم تتغير (اختبارات T8 كلها تمر دون تعديل) | ✅ `git diff --stat f3d94fa..0a181e2` = `main.rs` + `template.rs` + `devices.rs` فقط. لم أعدّل أي اختبار قديم؛ كامل الحزمة 335 ناجحاً، منها 330 كانت تمر قبل الإصلاح. اختبارات `template_*` و`order_*` و`raw_send_*` بلا تغيير |
| 2. لا `release_dedupe` مكرر بعدد الفروع، وكل خروج بخطأ يمر بنقطة واحدة | ✅ قراءة الكود: `release_dedupe` يُستدعى مرة واحدة داخل `release_dedupe_key`، وهي تُستدعى في موضعين فقط: ذراع `Err` بعد `deliver`، وفرع `delivered == 0`. أخطاء `tokens_for_uid`/`role_members`/`source_for`/`tokens_for_role`/`project_map` كلها `?` داخل `deliver` ⇒ `Err` ⇒ نفس نقطة التحرير |
| 3. الأسماء الخمسة موجودة، والتجربة السلبية موثقة، ولا `#![allow(dead_code)]` في `template.rs` | ✅ `cargo test -- --list` يعرض الأسماء الخمسة حرفياً؛ التجربة في §3؛ `grep` على `template.rs` لا يجد `#![allow(dead_code)]` |

## 5. انحرافات عن الأمر (وسببها)

1. **رمز فشل قراءة الأجهزة 500 وليس 503.** نصّت الخطوة على «`tokens_for_uid` يفشل ⇒ 503»، لكن `device_error(DeviceError::Database)` يعيد **500** `device storage failed` (بينما 503 لـ`Unavailable` فقط)، وهذا سلوك T6/T8 القائم ولم أغيّره لأنه خارج نطاق T8-F. الاختبار يؤكد 500 صراحة مع تعليق يوضح السبب؛ **جوهر المطلوب** (تحرير المفتاح) محقَّق ويثبته الجزء الثاني من الاختبار.
2. **لم أُعد أي `#[allow(dead_code)]` في `template.rs`.** الخطوة اشترطت إعادته لعنصر واحد «إن ظهر تحذير»؛ لم يظهر أي تحذير في `cargo build`، فلم يُضف شيء. ذكر المعامل `entity` في الخطوة لم يتحقق (يُستعمل داخل `render`).
3. **حذف كل allows السبعة في `devices.rs`** وليس بعضها: `cargo build` بلا تحذير أثبت أن كل العناصر صارت مستعملة فعلاً، وهذا هو شرط الخطوة («حيث صار العنصر مستعملاً فعلاً»).
4. **ملاحظة المفتاح الخدمي ومتابعات T8** سُجّلت في خطوة **T13** (قسم القوالب + بند «متابعات») لأنها خطوة التوثيق والحواجز؛ لم ألمس T10 كي لا أكرر النص في خطوتين.
5. **إعادة كتابة commitي T8-3/T8-4 سابقاً** بقيت كما هي؛ لا علاقة لها بهذا الإصلاح.

## 6. ملاحظات خارج النطاق (لم أعالجها هنا؛ سُجّلت في T13)

- `ConfigStore::save_template` يرفع `version`، و`RoleDirectory` يستعمل `snapshot.version`، فأي تعديل قالب يُبطل مخبأ الأدوار لكل التطبيق.
- `validate_template` يقبل `{var}` في `context.require` بينما المحرك يرفضه عند الإرسال (400 لكل طلب).
- `device_error(Database)` = 500 مقابل `Unavailable` = 503؛ التوحيد يحتاج قراراً (سؤال §7).
- `api_fallback` ما زال بلا مساري الأجهزة، و`PUBLISHED_COMMIT` في تقرير S0 ما زال فارغاً.

## 7. أسئلة للمالك/المراجع

1. هل تريد توحيد فشل قراءة الأجهزة/القاعدة إلى **503** (fail-closed كخدمة غير متاحة) بدل 500؟ التغيير سلوكي ويحتاج خطوة مستقلة؛ T8-F لم يغيّره.
2. هل `seconds` في `dedupe` قد تكون 0؟ حالياً `claim_dedupe(..., 0, now)` يجعل `expires_at == now`، فالمفتاح يُعتبر منتهياً فوراً ويُسمح بالتكرار. توثيق ذلك أو فرض حد أدنى يحتاج قراراً.

## 8. حالة git النهائية

```
$ git status --short
(نظيف بعد commit الوثائق)

$ git log --oneline -5
0a181e2 chore: T8-F drop stale dead_code allows
5d32ec1 fix(send): T8-F release the dedupe key on every non-delivering exit
f3d94fa docs(plan): T8 update execution tracker
07d9cab feat(send): T8-4 add POST /send/template
e3afebc feat(db): T8-3 add notification kind/ref, dedupe table and template usage touch
```

commit الوثائق (`docs(plan): T8-F update tracker`) يضيف هذا الملف و`T8-fix.md` وتحديث README والخطة.

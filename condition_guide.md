# FCM Condition Targeting Guide

Firebase Cloud Messaging (FCM) allows you to target devices based on boolean combinations of topics using the `condition` field. This guide explains what conditions are, how they work, and provides real-world examples.

---

## 1. Topic vs. Token vs. Condition

| Field | Purpose | Target Scope | Example Use Case |
|---|---|---|---|
| `token` | Direct device targeting | A single device | *"Your order #1234 has shipped"* |
| `topic` | Single topic broadcast | All devices subscribed to one topic | *"Breaking news for all subscribers"* |
| `condition` | Multi-topic logical targeting | Devices matching boolean logic (`&&`, `\|\|`, `!`) | *"Drivers in Iraq who opted in for promotions"* |

> **Rule:** Every `/send` request must contain **exactly one** of `topic`, `token`, or `condition`.

---

## 2. Syntax & Rules

FCM conditions evaluate whether a device is subscribed to specific topics using the `'<TopicName>' in topics` syntax:

- **Logical AND (`&&`):** The device must be subscribed to both topics.
- **Logical OR (`||`):** The device can be subscribed to either topic. (If subscribed to both, the device receives only **one** notification).
- **Logical NOT (`!`):** The device must NOT be subscribed to the specified topic.
- **Parentheses `()`:** Use parentheses to group conditions and control precedence.
- **Limit:** FCM supports combining up to **5 topics** in a single condition expression.

---

## 3. Real-World Examples

### Example 1: Logical AND (`&&`)
Target users who belong to **both** categories (e.g., Drivers located in Iraq):

```json
{
  "app": "watt",
  "condition": "'iraq' in topics && 'drivers' in topics",
  "title": "Road Alert in Baghdad",
  "body": "Special traffic alert for drivers operating in Iraq."
}
```

---

### Example 2: Logical OR (`||`)
Target users subscribed to **either** topic without sending duplicate notifications to users in both:

```json
{
  "app": "watt",
  "condition": "'discounts' in topics || 'flash_deals' in topics",
  "title": "Weekend Sale!",
  "body": "Get 20% off all products this Friday."
}
```

---

### Example 3: Logical NOT (`!`)
Broadcast to general news subscribers while excluding users who muted or opted out:

```json
{
  "app": "watt",
  "condition": "'news' in topics && !('opt_out' in topics)",
  "title": "Daily Roundup",
  "body": "Here are today's top highlights."
}
```

---

### Example 4: Complex Combined Condition (Parentheses)
Target customers in Iraq or Jordan who have enabled VIP offers:

```json
{
  "app": "watt",
  "condition": "('iraq' in topics || 'jordan' in topics) && 'vip_offers' in topics",
  "title": "Exclusive VIP Event",
  "body": "Special invitations available for VIP members in Iraq & Jordan."
}
```

---

## 4. How to Send via API

Make a `POST` request to `/api/v1/send` (or `/api/send`):

```bash
curl -X POST "https://fcm.subultech.top/api/v1/send" \
  -H "Authorization: Bearer <YOUR_API_KEY>" \
  -H "Content-Type: application/json" \
  -d '{
    "app": "watt",
    "condition": "'\''iraq'\'' in topics && '\''drivers'\'' in topics",
    "title": "Hello Drivers",
    "body": "Stay safe on the road!"
  }'
```

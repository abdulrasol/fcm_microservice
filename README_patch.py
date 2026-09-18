with open('/Users/rasol/DevsTools/codes/rust/fcm_microservice/README.md', 'r') as f:
    content = f.read()

import re

old_json = """**Body (JSON):**
```json
{
  "topic": "all",
  "title": "Welcome!",
  "body": "Thanks for joining us.",
  "image": "https://example.com/image.png",
  "data": {
    "action": "open_app",
    "user_id": 123
  }
}
```"""

new_json = """**Body (JSON):**
```json
{
  "topic": "all",
  "title": "Welcome!",
  "body": "Thanks for joining us.",
  "image": "https://example.com/image.png",
  "analytics_label": "welcome_campaign",
  "data": {
    "action": "open_app",
    "user_id": 123
  }
}
```"""

content = content.replace(old_json, new_json)

old_targets = """#### Supported Targets (Use exactly one):
- `"topic": "topic_name"` (e.g., "admins")
- `"token": "device_fcm_token_here"`
- `"condition": "'dogs' in topics || 'cats' in topics"`"""

new_targets = """#### Supported Targets (Use exactly one):
- `"topic": "topic_name"` (e.g., "admins")
- `"token": "device_fcm_token_here"`
- `"condition": "'dogs' in topics || 'cats' in topics"`

#### Optional Fields:
- `"body"`: Text body of the notification.
- `"image"`: URL to an image.
- `"data"`: Custom JSON object. It will be safely converted to string key/value pairs to comply with FCM v1.
- `"analytics_label"`: Attaches an `fcm_options: { "analytics_label": "..." }` to the notification to natively hook into Google Analytics 4 (GA4) for delivery/open tracking."""

content = content.replace(old_targets, new_targets)

with open('/Users/rasol/DevsTools/codes/rust/fcm_microservice/README.md', 'w') as f:
    f.write(content)
print("Updated README")

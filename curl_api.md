BASE="https://fcm.subultech.top/api/v1"
APP="<EXISTING_APP_NAME>"
OLD_API_KEY="<OLD_API_KEY>"
SERVICE_API_KEY="<SERVICE_API_KEY>"
ADMIN_ID_TOKEN="<FIREBASE_ADMIN_ID_TOKEN>"
NON_ADMIN_ID_TOKEN="<FIREBASE_NON_ADMIN_ID_TOKEN>"
DEVICE_TOKEN="<DEVICE_FCM_TOKEN>"

# 1) بلا ترويسة => 401
curl -i "$BASE/apps"

# 2) المفتاح القديم مع LEGACY_API_KEY_ENABLED=false => 401 (استخدم app موجوداً)
curl -i -X POST "$BASE/send" \
  -H "Authorization: Bearer $OLD_API_KEY" \
  -H "Content-Type: application/json" \
  -d "{\"app\":\"$APP\",\"topic\":\"all\",\"title\":\"legacy-disabled-check\"}"

# 3) ID token لمستخدم غير أدمن => 403
curl -i -X POST "$BASE/send" \
  -H "Authorization: Bearer $NON_ADMIN_ID_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"app\":\"$APP\",\"topic\":\"all\",\"title\":\"non-admin-check\"}"

# 4) ID token لأدمن => 200
curl -i -X POST "$BASE/send" \
  -H "Authorization: Bearer $ADMIN_ID_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"app\":\"$APP\",\"topic\":\"all\",\"title\":\"admin-check\"}"

# 5) subscribe لموضوع عام بلا توكن => 200
curl -i -X POST "$BASE/topics/subscribe" \
  -H "Content-Type: application/json" \
  -d "{\"app\":\"$APP\",\"topic\":\"all\",\"tokens\":[\"$DEVICE_TOKEN\"]}"

# 6) subscribe لـ admins بلا توكن => 403
curl -i -X POST "$BASE/topics/subscribe" \
  -H "Content-Type: application/json" \
  -d "{\"app\":\"$APP\",\"topic\":\"admins\",\"tokens\":[\"$DEVICE_TOKEN\"]}"

# 7) مفتاح الخدمة => 200
curl -i "$BASE/apps" \
  -H "Authorization: Bearer $SERVICE_API_KEY"
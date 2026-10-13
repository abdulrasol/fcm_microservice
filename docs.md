# FCM Microservice & Dashboard API Documentation

Welcome to the documentation for the FCM (Firebase Cloud Messaging) Microservice. This service allows you to send push notifications to multiple Firebase projects from a single unified API, track notification history, and manage the system via an integrated web dashboard.

## Base URL
All API endpoints support both `/api` and `/api/v1` prefixes.
- **V1 (Recommended):** `http://localhost:8080/api/v1`
- **Standard:** `http://localhost:8080/api`

For production deployment (e.g. `https://fcm.subultech.top`):
- `https://fcm.subultech.top/api/v1` or `https://fcm.subultech.top/api`

## Authentication
All API endpoints (except `/auth/login` and static dashboard files) are protected and require a Bearer token.
Pass your `API_KEY` (defined in your `.env` file) in the HTTP headers:
```
Authorization: Bearer <YOUR_API_KEY>
```

---

## Firebase Setup & Credentials
To use this microservice, you need to provide Firebase Service Account credentials for each app you want to manage.

1. Go to the [Firebase Console](https://console.firebase.google.com/).
2. Select your Firebase project.
3. Navigate to **Project Settings** (the gear icon) > **Service accounts**.
4. Click **Generate new private key** and download the JSON file.
5. Rename the downloaded file to something short and identifiable (e.g., `create_app.json`). **The filename (without `.json`) will be used as the `app` identifier in all API requests.**
6. Place this file inside the `credentials/` folder in the root of this project.

*You can add as many service account JSON files as you want to the `credentials/` folder. The system will automatically detect them.*

---

## 1. Send Notification
**Endpoints:** `POST /api/v1/send` or `POST /api/send`  
**Description:** Sends a push notification to a specific Firebase project (app). It automatically logs the notification to the SQLite history database.

### Request Payload (JSON)
| Field | Type | Required | Description |
|---|---|---|---|
| `app` | String | **Yes** | The exact name of the target app's credentials file (without the `.json` extension) located in the `credentials/` folder. Example: `create_app`. |
| `topic` | String | No* | The FCM topic to send the notification to. |
| `token` | String | No* | A specific device FCM token to send to. |
| `condition` | String | No* | FCM condition for boolean topic targeting (e.g., `'TopicA' in topics && 'TopicB' in topics`). See [Condition Guide](condition_guide.md) for full syntax & examples. |
| `title` | String | **Yes** | The title of the notification. |
| `body` | String | No | The body text of the notification. |
| `image` | String | No | URL of an image to display in the notification. |
| `data` | Object | No | Custom key-value pairs to send silently to the app. |
| `analytics_label` | String | No | Label used for Firebase Analytics. |

*\*Note: You must provide exactly one of `topic`, `token`, or `condition`. For details on building multi-topic conditions, check [condition_guide.md](condition_guide.md).*

### Example Request
```json
{
  "app": "create_app",
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
```

### Example Responses
**200 OK** (Success)
```json
{
  "status": "success",
  "response": "projects/create_app/messages/837263829373..."
}
```

**400 Bad Request** (Missing App or invalid targeting)
```json
{
  "error": "The 'app' field is required. E.g., 'app': 'create_app'"
}
```

**404 Not Found** (App credentials not found)
```json
{
  "error": "App 'create_app' not found in credentials folder."
}
```

---

## 2. List Available Apps
**Endpoints:** `GET /api/v1/apps` or `GET /api/apps`  
**Description:** Returns a list of all Firebase projects registered with the system. It dynamically reads the `credentials/` directory.

### Example Response
**200 OK**
```json
[
  "create_app",
  "store_app",
  "driver_app"
]
```

---

## 3. Login (Dashboard Use)
**Endpoints:** `POST /api/v1/auth/login` or `POST /api/auth/login`  
**Description:** Validates dashboard admin credentials against the `.env` file and returns the `API_KEY`.

### Request Payload (JSON)
```json
{
  "email": "admin@example.com",
  "password": "secure_password"
}
```

### Example Response
**200 OK**
```json
{
  "token": "your_secure_api_key_here"
}
```

---

## 4. Get Notification History
**Endpoints:** `GET /api/v1/history` or `GET /api/history`  
**Description:** Retrieves the log of previously sent notifications from the SQLite database with optional filtering and pagination limits.

### Query Parameters
| Parameter | Type | Required | Description |
|---|---|---|---|
| `app` | String | No | Filter history records by specific app (e.g., `?app=watt`). |
| `topic` | String | No | Filter by target topic or token substring (e.g., `?topic=admins`). |
| `search` | String | No | Search keyword matched against notification `title` or `payload` body (e.g., `?search=Order`). |
| `limit` | Integer | No | Max number of history items to return (default: `100`). |

### Example Request
```
GET /api/v1/history?app=watt&topic=admins&limit=50
```

### Example Response
**200 OK**
```json
[
  {
    "id": 1,
    "app": "create_app",
    "target": "admins",
    "title": "Welcome!",
    "status": "success",
    "created_at": "2026-10-13 10:00:00",
    "payload": "{\"app\":\"create_app\",\"topic\":\"admins\",\"title\":\"Welcome!\"}"
  }
]
```

---

## 5. Manage Topics
**Endpoints:** `GET /api/v1/topics` or `GET /api/topics`  
**Description:** Retrieves saved topics for quick access in the dashboard, optionally filtered by app.

### Query Parameters
- `app` (String, Optional): Filter topics for a specific app (e.g., `?app=watt`).

**Endpoints:** `POST /api/v1/topics` or `POST /api/topics`  
**Description:** Saves a new topic for a specific app to the SQLite database.
### Request Payload
```json
{
  "app": "create_app",
  "name": "promo_users"
}
```

---

## 6. Subscribe to Topic
**Endpoint:** `POST /topics/subscribe`  
**Description:** Subscribes one or more FCM device tokens to a specific topic.

### Request Payload (JSON)
```json
{
  "app": "create_app",
  "topic": "promo_users",
  "tokens": [
    "device_token_1",
    "device_token_2"
  ]
}
```
- `app` (String, **Required**): The exact name of the target app's credentials file (without `.json`).
- `topic` (String, **Required**): The topic name (without `/topics/` prefix, max 900 characters).
- `tokens` (Array of Strings, **Required**): List of device registration tokens to subscribe (up to 1000 tokens per request).

### Example Response
**200 OK**
```json
{
  "success_count": 2,
  "failure_count": 0,
  "errors": []
}
```

---

## 7. Unsubscribe from Topic
**Endpoint:** `POST /topics/unsubscribe`  
**Description:** Unsubscribes one or more FCM device tokens from a specific topic.

### Request Payload (JSON)
```json
{
  "app": "create_app",
  "topic": "promo_users",
  "tokens": [
    "device_token_1",
    "device_token_2"
  ]
}
```
- `app` (String, **Required**): The exact name of the target app's credentials file (without `.json`).
- `topic` (String, **Required**): The topic name (without `/topics/` prefix).
- `tokens` (Array of Strings, **Required**): List of device registration tokens to unsubscribe (up to 1000 tokens per request).

### Example Response
**200 OK**
```json
{
  "success_count": 2,
  "failure_count": 0,
  "errors": []
}
```

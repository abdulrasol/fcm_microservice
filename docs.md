# FCM Microservice & Dashboard API Documentation

Welcome to the documentation for the FCM (Firebase Cloud Messaging) Microservice. This service allows you to send push notifications to multiple Firebase projects from a single unified API, track notification history, and manage the system via an integrated web dashboard.

## Base URL
All API endpoints are prefixed with `/api/v1`.
If running locally, the base URL is `http://localhost:8080/api/v1`.

## Authentication
All API endpoints (except `/auth/login` and static dashboard files) are protected and require a Bearer token.
Pass your `API_KEY` (defined in your `.env` file) in the HTTP headers:
```
Authorization: Bearer <YOUR_API_KEY>
```

---

## 1. Send Notification
**Endpoint:** `POST /send`  
**Description:** Sends a push notification to a specific Firebase project (app). It automatically logs the notification to the SQLite history database.

### Request Payload (JSON)
| Field | Type | Required | Description |
|---|---|---|---|
| `app` | String | **Yes** | The exact name of the target app's credentials file (without the `.json` extension) located in the `credentials/` folder. Example: `create_app`. |
| `topic` | String | No* | The FCM topic to send the notification to. |
| `token` | String | No* | A specific device FCM token to send to. |
| `condition` | String | No* | FCM condition for sending (e.g., `'TopicA' in topics`). |
| `title` | String | **Yes** | The title of the notification. |
| `body` | String | No | The body text of the notification. |
| `image` | String | No | URL of an image to display in the notification. |
| `data` | Object | No | Custom key-value pairs to send silently to the app. |
| `analytics_label` | String | No | Label used for Firebase Analytics. |

*\*Note: You must provide exactly one of `topic`, `token`, or `condition`.*

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
**Endpoint:** `GET /apps`  
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
**Endpoint:** `POST /auth/login`  
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
**Endpoint:** `GET /history`  
**Description:** Retrieves the log of previously sent notifications from the SQLite database.

### Example Response
**200 OK**
```json
[
  {
    "id": 1,
    "app": "create_app",
    "target": "topic:all",
    "title": "Welcome!",
    "status": "success",
    "created_at": "2026-09-29T10:00:00Z"
  }
]
```

---

## 5. Manage Topics
**Endpoint:** `GET /topics`  
**Description:** Retrieves saved topics for quick access in the dashboard.

**Endpoint:** `POST /topics`  
**Description:** Saves a new topic to the SQLite database.
**Payload:** `{"name": "promo_users"}`

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

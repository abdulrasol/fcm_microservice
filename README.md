# FCM V1 Microservice (Rust) 🦀

A lightweight, blazing-fast, and secure microservice written in **Rust** to send push notifications via the new **Firebase Cloud Messaging HTTP v1 API**.

## 🌟 Why this microservice?
Google has deprecated the **Legacy FCM API**. The new **HTTP v1 API** requires OAuth2 tokens generated from a `service-account.json`. 
Generating these tokens directly on client apps (iOS/Android/Web) is a massive security risk. This microservice acts as a highly optimized, ultra-lightweight proxy:
- Your app sends a simple request with a static `API_KEY` to this service.
- This service handles Google OAuth2 authentication, securely generates the Bearer token, maps your payload to the FCM v1 format, and dispatches the notification to Firebase.

## 🚀 Features
- **Ultra-lightweight:** The Docker image size is incredibly small (~15MB) and consumes less than 10MB of RAM.
- **Secure:** Endpoint is protected by an `Authorization: Bearer <API_KEY>`.
- **Automatic Token Management:** Automatically generates and caches the OAuth2 token for Firebase using `yup-oauth2`.
- **Flexible Targeting:** Send notifications to a `topic`, a specific `token`, or a `condition`.
- **Smart Data Mapping:** Automatically formats custom `data` payloads to string-only values to comply with FCM v1 strict rules.

---

## 🛠️ How to setup and deploy

### 1. Prerequisites
- [Docker](https://docs.docker.com/get-docker/) installed on your server.
- A Firebase Project.

### 2. Get your Firebase Service Account
1. Go to your [Firebase Console](https://console.firebase.google.com/).
2. Navigate to **Project Settings** (the gear icon) -> **Service accounts**.
3. Click on **Generate new private key** and save the `.json` file.
4. Rename the downloaded file to `service-account.json` and place it in the root directory of this project.

### 3. Configure the Environment
Copy the example environment file and create your own `.env`:
```bash
cp .env.example .env
```
Edit `.env` and fill in your details:
```env
PORT=8080
API_KEY=my_super_secret_api_key_123   # Create a secure password here
FIREBASE_PROJECT_ID=your-project-id   # Find this in your Firebase Console
GOOGLE_APPLICATION_CREDENTIALS=/app/service-account.json
```

### 4. Run the Service (Docker)
Start the service using Docker Compose. It will build the multi-stage Alpine Rust image and start the server.
```bash
docker-compose up -d --build
```
*Your service is now running on port 8080!*

---

## 📡 API Usage

### Endpoint: `POST /api/v1/send`

**Headers:**
```http
Content-Type: application/json
Authorization: Bearer my_super_secret_api_key_123
```

**Body (JSON):**
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
```

#### Supported Targets (Use exactly one):
- `"topic": "topic_name"` (e.g., "admins")
- `"token": "device_fcm_token_here"`
- `"condition": "'dogs' in topics || 'cats' in topics"`

#### Optional Fields:
- `"body"`: Text body of the notification.
- `"image"`: URL to an image.
- `"data"`: Custom JSON object. It will be safely converted to string key/value pairs to comply with FCM v1.
- `"analytics_label"`: Attaches an `fcm_options: { "analytics_label": "..." }` to the notification to natively hook into Google Analytics 4 (GA4) for delivery/open tracking.

#### Example using `cURL`:
```bash
curl -X POST http://localhost:8080/api/v1/send \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer my_super_secret_api_key_123" \
  -d '{
    "topic": "all",
    "title": "Hello from Rust!",
    "body": "This is a lightning fast notification."
  }'
```

## 💖 Contributing
Feel free to open issues and pull requests to improve the microservice.

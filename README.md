# FCM V1 Microservice (Rust) 🦀

A lightweight, blazing-fast, and secure microservice written in **Rust** to send push notifications via the new **Firebase Cloud Messaging HTTP v1 API**.

## 🌟 Why this microservice?
Google has deprecated the **Legacy FCM API**. The new **HTTP v1 API** requires OAuth2 tokens generated from a `service-account.json`. 
Generating these tokens directly on client apps (iOS/Android/Web) is a massive security risk. This microservice acts as a highly optimized, ultra-lightweight proxy:
- Your trusted application backend sends requests with a private `API_KEY` to this service. Never embed this key in browser, mobile, or distributed desktop clients.
- This service handles Google OAuth2 authentication, securely generates the Bearer token, maps your payload to the FCM v1 format, and dispatches the notification to Firebase.

## 🚀 Features
- **Docker deployment:** Build and run the service using the included Dockerfile and Compose configuration.
- **Secure:** Endpoint is protected by an `Authorization: Bearer <API_KEY>`.
- **Automatic Token Management:** Automatically generates and caches the OAuth2 token for Firebase using `yup-oauth2`.
- **Topic management:** Subscribe/unsubscribe up to 1000 tokens and inspect per-token results.
- **Optional browser access:** Configure explicit allowed origins with `CORS_ALLOWED_ORIGINS`.
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
If `.env` does not exist, copy the example file once (keep existing credentials when updating):
```bash
cp .env.example .env
```
Edit `.env` and fill in your details:
```env
PORT=8080
API_KEY=my_super_secret_api_key_123   # Create a secure password here
FIREBASE_PROJECT_ID=your-project-id   # Find this in your Firebase Console
GOOGLE_APPLICATION_CREDENTIALS=/app/service-account.json
CORS_ALLOWED_ORIGINS=https://your-app.example.com,http://localhost:62120
```

### 4. Run the Service (Docker)
Start the service using Docker Compose. It will build the multi-stage Alpine Rust image and start the server.
```bash
docker compose up -d --build
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

## Topic subscriptions (web, mobile, and other FCM clients)

Sending to `all` only reaches tokens subscribed to a topic named `all`. It is not
an automatic broadcast to every device. Topic names are application-defined;
this service has no built-in roles, user database, or special `admins` behavior.
Each deployment uses one Firebase project/service account. Deploy separate
instances for different Firebase projects; this is not a multi-tenant gateway.

### Endpoints

- `POST /api/v1/topics/subscribe`
- `POST /api/v1/topics/unsubscribe`

Both require the same `Authorization: Bearer <API_KEY>` header as sending:

```json
{
  "topic": "product-updates",
  "tokens": ["FCM_REGISTRATION_TOKEN_1", "FCM_REGISTRATION_TOKEN_2"]
}
```

Use a bare topic name, without `/topics/`. Allowed characters: ASCII letters,
digits, `-`, `_`, `.`, `~`, `%`; length 1–900. Supply 1–1000 nonempty tokens per
request. Tokens must belong to the configured Firebase project. Larger batches
must be split by the caller. Results refer to the original input indexes.

### Subscribe devices: `POST /api/v1/topics/subscribe`

Adds the supplied device/browser tokens to one topic. This operation does not
send a notification. Run it from your trusted backend or admin terminal.

**Required fields:**
- `topic`: The group name, for example `all` or `product-updates`.
- `tokens`: An array of FCM registration tokens, even when subscribing one device.
  These are device/browser tokens, not the VAPID key or a user's login token.

Replace `my_super_secret_api_key_123` with your configured `API_KEY`, and replace
`device_fcm_token_here` with the actual token returned by Firebase Messaging.

#### Example using `cURL`:

```bash
curl -X POST http://localhost:8080/api/v1/topics/subscribe \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer my_super_secret_api_key_123" \
  -d '{
    "topic": "all",
    "tokens": ["device_fcm_token_here"]
  }'
```

#### Successful response:

```json
{
  "success_count": 1,
  "failure_count": 0,
  "errors": []
}
```

To subscribe several devices in one request, pass all their tokens in `tokens`:

```bash
curl -X POST http://localhost:8080/api/v1/topics/subscribe \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer my_super_secret_api_key_123" \
  -d '{
    "topic": "product-updates",
    "tokens": ["first_device_fcm_token", "second_device_fcm_token"]
  }'
```

### Send to subscribed devices: `POST /api/v1/send`

After subscribing the device to `all`, send a notification to the same topic:

```bash
curl -X POST http://localhost:8080/api/v1/send \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer my_super_secret_api_key_123" \
  -d '{
    "topic": "all",
    "title": "Hello from Rust!",
    "body": "This notification goes to devices subscribed to all."
  }'
```

### Unsubscribe devices: `POST /api/v1/topics/unsubscribe`

Removes the supplied tokens from the specified topic. It does not delete the
tokens or remove their memberships in other topics. Use the same `topic` and
`tokens` fields as subscription.

#### Example using `cURL`:

```bash
curl -X POST http://localhost:8080/api/v1/topics/unsubscribe \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer my_super_secret_api_key_123" \
  -d '{
    "topic": "all",
    "tokens": ["device_fcm_token_here"]
  }'
```

#### Successful response:

```json
{
  "success_count": 1,
  "failure_count": 0,
  "errors": []
}
```

After Firebase applies the change, new messages sent to `all` no longer target
this device through that topic. Direct messages to its token remain possible.

### Understanding subscription results

- `success_count`: Number of tokens successfully processed.
- `failure_count`: Number of tokens that failed.
- `errors`: Failed entries with a zero-based `index` into your `tokens` array
  and Firebase's `error` code. For example, index `1` means the second token.

HTTP 200 can include per-token failures:

```json
{
  "success_count": 1,
  "failure_count": 1,
  "errors": [{"index": 1, "error": "NOT_FOUND"}]
}
```

Cache subscription success only after the requested token is acknowledged
(`success_count: 1`, `failure_count: 0` for one token). A timeout, network error,
or failed token result must not create a successful local cache entry. Delete
the subscription cache only after successful unsubscription.

Always inspect `failure_count`. Remove stale tokens for `NOT_FOUND`; investigate
`INVALID_ARGUMENT` rather than repeatedly retrying it. Retry transient errors
such as `INTERNAL` or `RESOURCE_EXHAUSTED` with bounded exponential backoff.
Invalid topic/batch values return 400; malformed JSON/type errors are rejected by
Axum (400/422), invalid credentials return 401, and upstream failures return 502.
The default Axum JSON body limit also applies. Do not treat a 502 as confirmation
that no tokens changed: retrying the same membership operation is appropriate.

Sending remains on FCM HTTP v1. Topic membership uses Google's separate
[topic batch API](https://developers.google.com/instance-id/reference/server),
with short-lived OAuth credentials and `access_token_auth: true`, not a legacy
FCM server key. No additional environment variables are needed.

### Integration architecture

```text
Browser / mobile app → your authenticated application backend → this service → Firebase
```

1. The client obtains an FCM registration token and sends it to **your backend**
   with the user's normal session or Firebase ID token.
2. Your backend verifies that authentication, associates the device token with
   the user, and decides allowed topics from trusted roles/preferences.
3. Your backend calls this service's subscribe endpoint with its private API key.
4. Send notifications through `/api/v1/send` using the topic as usual.
5. On logout, account switching, permission revocation, or role changes, your
   backend removes obsolete memberships. On token refresh, register the new
   token, reconcile subscriptions, and remove the old token where available.

Do not expose this service's API key to clients. A holder of the key can send
notifications and manage any topic. This service intentionally does not verify
end-user JWTs or interpret application roles. Never trust a client-supplied role
or unrestricted topic name. Topics are not a confidentiality boundary: use
authorized device-token targeting for sensitive user/admin messages.

### Flutter Web setup

1. Enable Firebase Cloud Messaging API for the project and configure a service
   account permitted to send/manage FCM messaging. Keep its JSON key on the server.
2. Firebase Console → Project settings → Cloud Messaging → Web Push certificates:
   generate/copy the **public VAPID key**.
3. Publish `/firebase-messaging-sw.js`, initializing Firebase for the same project.
   Serve it as JavaScript (not an SPA HTML fallback) on HTTPS, or localhost in
   development. See [Flutter FCM setup](https://firebase.google.com/docs/cloud-messaging/flutter/get-started)
   and [background handling](https://firebase.google.com/docs/cloud-messaging/flutter/receive-messages).
4. From a user action, check browser support, request notification permission,
   then call `FirebaseMessaging.instance.getToken(vapidKey: publicVapidKey)`.
5. Send that token to your application backend using the architecture above.
   Listen to `onTokenRefresh` and update the backend when it changes.
6. Handle foreground messages through `FirebaseMessaging.onMessage`. Background
   notification payloads are handled by the messaging service worker. Data-only
   messages need your own background display/processing logic.

Flutter's browser implementation cannot directly call `subscribeToTopic`.
Browser access can be enabled with `CORS_ALLOWED_ORIGINS`; it is disabled by
default. See [Browser access and CORS configuration](#browser-access-and-cors_allowed_origins)
below. CORS does not replace application authentication or admin authorization.

### Deployment and verification

- With Docker Compose, keep `GOOGLE_APPLICATION_CREDENTIALS=/app/service-account.json`;
  the existing read-only volume mounts the local file there.
- For a local Rust run, use an absolute local credentials path instead and run
  `cargo run --release`. Configure `PORT`, `API_KEY`, and `FIREBASE_PROJECT_ID` as above.
- Enable Cloud Messaging API (HTTP v1), use credentials for the selected project,
  and permit outbound HTTPS to Google's OAuth, FCM, and `iid.googleapis.com` services.
- Use a strong API key, keep `.env` and service-account files private, and expose
  the service only to trusted backends through private networking or HTTPS.
- Rebuild/restart after updating: `docker compose up -d --build`.
- Check `/health`, obtain a real test browser token, subscribe it to a test topic,
  send to that topic, and verify receipt with the browser in the background.
  Unsubscribe it, send again, and confirm it no longer receives new test messages
  after Firebase applies the change. Direct token sending helps isolate delivery
  issues from membership issues.
- Run local validation with `cargo fmt --check` and `cargo test`.
  Unit tests do not contact Firebase; live delivery requires your own credentials
  and test device/browser.

## Browser access and `CORS_ALLOWED_ORIGINS`

### Where to put it

Add this variable to **`.env` in the root of this service**, alongside
`Cargo.toml` and `docker-compose.yml`. It is not a Flutter setting and does not go
in `src/main.rs`. Example project layout:

```text
fcm_microservice/
├── .env
├── .env.example
├── Cargo.toml
├── docker-compose.yml
└── src/
```

On your deployed server, edit the `.env` inside that server's checkout of this
project. Editing your Mac's file alone does not change the deployed service.
If your hosting platform manages environment variables, set the same variable
in the service's environment settings instead.

If `.env` does not exist, copy `.env.example` once. Do not overwrite an existing
`.env` containing your credentials. Keep the other existing values and add:

```env
# Production browser application
CORS_ALLOWED_ORIGINS=https://your-app.example.com
```

For production plus local development:

```env
CORS_ALLOWED_ORIGINS=https://your-app.example.com,http://localhost:62120
```

These domains are examples: anyone deploying this service should substitute
**their frontend origins**, not the notification API's domain. An origin consists
of scheme + hostname + optional port. Do not include routes such as `/login`,
query strings, or fragments. Use a comma-separated list, without a wildcard `*`.
`http://localhost:62120` and `http://localhost:50122` are different origins;
`127.0.0.1` and `localhost` are also different hosts.

For Flutter local testing, fix the port to match the example:

```bash
flutter run -d chrome --web-port=62120
```

When the variable is absent or empty, cross-origin browser access is not enabled.
Server-to-server requests and curl do not require CORS permission.

### Complete Docker environment example

```env
PORT=8080
API_KEY=replace_with_your_private_api_key
FIREBASE_PROJECT_ID=your-firebase-project-id
GOOGLE_APPLICATION_CREDENTIALS=/app/service-account.json
CORS_ALLOWED_ORIGINS=https://your-app.example.com,http://localhost:62120
```

The Compose file already loads `.env` using `env_file`. Keep the credentials path
above for Docker: the read-only volume mounts your local `service-account.json`
at that container path. Never publish your `.env` or service account credentials.

### Apply the change

Run from the service's directory on the deployment server:

```bash
docker compose up -d --build --force-recreate fcm_microservice
docker compose logs --tail=50 fcm_microservice
```

Recreating the container loads the updated environment; a plain container restart
does not update its configured environment. Rebuilding also includes the new Rust
CORS and subscription code. The current Compose mapping is `8080:8080`: both
the host and container use port **8080**, matching all localhost curl examples.
If you change the host mapping (for example to `8084:8080`), use that host port
in your curl commands and reverse-proxy configuration. Keep `PORT=8080` inside
the container unless you also update the container port in Compose.

For a direct Rust run, set `GOOGLE_APPLICATION_CREDENTIALS` to your actual local
JSON path in `.env`, stop the previous process, then run from the project root:

```bash
cargo run --release
```

The app loads `.env` at startup. Existing process environment variables take
precedence over `.env`; update them too if your service manager sets them.

### Verify browser preflight

Test the deployed endpoint (substitute your own domains):

```bash
curl -i -X OPTIONS https://notifications.example.com/api/v1/topics/subscribe \
  -H "Origin: https://your-app.example.com" \
  -H "Access-Control-Request-Method: POST" \
  -H "Access-Control-Request-Headers: authorization,content-type"
```

The response should include `Access-Control-Allow-Origin` matching the supplied
allowed origin, plus allowed methods containing POST and allowed headers
containing authorization and content-type. Preflight does not need an API key;
the subsequent POST still needs `Authorization: Bearer <API_KEY>`.

The CORS configuration applies to:

- `POST /api/v1/send`
- `POST /api/v1/notification/send-topic` (existing alias)
- `POST /api/v1/topics/subscribe`
- `POST /api/v1/topics/unsubscribe`

The middleware handles OPTIONS and allows GET/POST with Authorization and
Content-Type. Do not add unrelated custom headers to browser requests unless
you also deliberately add them to the server's allowed headers.

If it still fails, check the browser's actual Origin (including its local port),
confirm you recreated the deployed container, and check the reverse proxy.
The proxy must forward OPTIONS and must not add duplicate/conflicting CORS
headers. An HTTP 401 on POST means the API key was rejected; it is not fixed by
adding more allowed origins. Invalid origin configuration causes startup to fail
with a CORS configuration message in the logs.

### CORS and admin permissions are separate

CORS permits a browser origin to read responses. It does **not** verify the
logged-in user, enforce an admin role, or protect an API key embedded in Flutter
Web. Anyone who obtains the service key can send and manage topics. For an
admin-only production feature, keep the key in your application backend, verify
the user's session/role there, and let that backend call this general service.


## 💖 Contributing

Feel free to open issues and pull requests to improve the microservice.

# FCM Microservice & Dashboard API Documentation

Welcome to the documentation for the FCM (Firebase Cloud Messaging) Microservice. This service sends push notifications to multiple Firebase projects from a single API, tracks notification history, and manages saved topics.

## Base URL

All API endpoints are served under `/api/v1`:

- **Local:** `http://localhost:8080/api/v1`
- **Production:** `https://fcm.subultech.top/api/v1`

> **Smart Fallback:** Any invalid endpoint under `/api/*` or `/api/v1/*` returns a `404 Not Found` JSON listing the available endpoints.

---

## Authentication & Authorization

No client application ships a shared secret anymore. There are three credential types:

| Credential | Header | Purpose |
|---|---|---|
| **Firebase ID token** | `Authorization: Bearer <id_token>` | End-user / admin identity. Obtained from the Firebase Auth SDK on the client. |
| **Service API key** | `Authorization: Bearer <SERVICE_API_KEY>` | Server-to-server integrations only. Configured outside the repository. |
| **Legacy API key** (temporary) | `Authorization: Bearer <API_KEY>` | Only when `LEGACY_API_KEY_ENABLED=true`, and only on `/send`. |

### Firebase ID token verification

The service verifies ID tokens locally:

- RS256 signature against Google's public JWKs from
  `https://www.googleapis.com/service_accounts/v1/jwk/securetoken@system.gserviceaccount.com`
  (cached in memory according to `Cache-Control: max-age`; an unknown `kid` triggers at most one refresh per minute).
- `alg` must be `RS256`.
- `iss` must be `https://securetoken.google.com/<project_id>`.
- `aud` must be `<project_id>`.
- `exp` and `iat` are checked with **60 seconds** of clock leeway.
- `sub` must be non-empty.
- `auth_time` must not be in the future.

`project_id` is always taken from the server-side credentials map for the requested `app`.
A `project_id` supplied in the request body is ignored.

### Admin determination

After the token is verified, the service reads the Firestore document
`users/{uid}` in the app's Firebase project using that app's service account:

```
GET https://firestore.googleapis.com/v1/projects/<project_id>/databases/(default)/documents/users/<uid>
```

The user is an admin only when `fields.role.stringValue == "admin"`.
Results are cached in memory for 60 seconds per `(app, uid)`, with a bounded cache size.

If Firestore cannot be reached or the service account lacks permission, privileged
requests fail closed with **503 Service Unavailable**. There is no "allow by default".

**IAM requirement:** the service account used for the app must be able to read
Firestore. The least-privilege role is **Cloud Datastore Viewer** (`roles/datastore.viewer`)
on the Firebase project. It also needs the Firebase Cloud Messaging permissions already
required for sending.

### Endpoint permissions

| Endpoint | Firebase ID token | `SERVICE_API_KEY` | Legacy `API_KEY` |
|---|---|---|---|
| `POST /send` | Admin of `app` | Yes (all apps) | Only if enabled, and only this route |
| `GET /apps` | Any registered project; returns only apps where the user is admin | Yes (all apps) | No |
| `GET /history` | Admin of `app` (or of the apps returned when `app` is omitted) | Yes (all history) | No |
| `GET /topics` | Admin of `app` / of each returned app | Yes | No |
| `POST /topics` | Admin of `app` | Yes | No |
| `POST /topics/subscribe` | Topic policy below | Admin topic access | No |
| `POST /topics/unsubscribe` | Topic policy below | Admin topic access | No |
| `GET /health` | Public | Public | Public |

An ID token whose audience is not a registered app is rejected with `401`.
A valid ID token for a user who is not an admin is rejected with `403`.

### Topic access policy (subscribe / unsubscribe)

| Topic | Anonymous (no/invalid token) | Any valid ID token | Admin / service key |
|---|---|---|---|
| `all` | Allowed | Allowed | Allowed |
| `country_XX` (exactly two ASCII letters) | Allowed | Allowed | Allowed |
| `lang_XX` (exactly two ASCII letters) | Allowed | Allowed | Allowed |
| `users`, `companies`, `technicals` | Forbidden (403) | Allowed | Allowed |
| `admins` | Forbidden (403) | Forbidden (403) | Allowed |
| Any other valid topic name | Forbidden (403) | Forbidden (403) | Allowed |

- A missing or invalid `Authorization` header is **ignored** on subscribe/unsubscribe,
  so the request continues as anonymous. It is never interpreted as an admin credential.
- The legacy `API_KEY` is never accepted on these routes.
- Privileged topics require a valid admin ID token or the service key.

### Legacy key (transitional only)

`LEGACY_API_KEY_ENABLED` defaults to **false**. When explicitly set to `true`:

- `API_KEY` is accepted **only** on `POST /send`.
- Every successful use emits a `tracing::warn!` log entry. The key is never logged.
- It is never accepted on `/history`, `/apps` or `/topics`.
- Use this only for the few days needed to update the admin application.

---

## Firebase Setup & Credentials

1. Go to the [Firebase Console](https://console.firebase.google.com/).
2. Select the project.
3. **Project Settings** > **Service accounts** > **Generate new private key**.
4. Rename the downloaded file to a short identifier (e.g. `create_app.json`).
   **The filename without `.json` is the `app` identifier used in requests.**
5. Place it in `credentials/` on the server. Never commit it.
6. Grant the service account `roles/datastore.viewer` (or broader) so admin role lookups work.

The service watches `credentials/` and loads new files automatically.

---

## 1. Send Notification

**Endpoint:** `POST /api/v1/send`
**Authorization:** Admin ID token for `app`, or `SERVICE_API_KEY` (legacy key only when enabled)

### Request Payload (JSON)

| Field | Type | Required | Description |
|---|---|---|---|
| `app` | String | **Yes** | Credentials file name without `.json`. |
| `topic` | String | No* | FCM topic. |
| `token` | String | No* | Single device registration token. |
| `condition` | String | No* | Boolean topic condition. See [Condition Guide](condition_guide.md). |
| `title` | String | **Yes** | Notification title (max 200 characters). |
| `body` | String | No | Body text (max 4000 characters). |
| `image` | String | No | `https://` image URL (max 2048 characters). |
| `data` | Object | No | Silent key/value data (max 4 KB serialized). |
| `analytics_label` | String | No | Firebase Analytics label. |

\* Provide **exactly one** of `topic`, `token` or `condition`.

### Example Request

```bash
curl -X POST "https://fcm.subultech.top/api/v1/send" \
  -H "Authorization: Bearer <FIREBASE_ID_TOKEN>" \
  -H "Content-Type: application/json" \
  -d '{
    "app": "create_app",
    "topic": "all",
    "title": "Welcome!",
    "body": "Thanks for joining us.",
    "image": "https://example.com/image.png",
    "data": { "action": "open_app", "user_id": "123" }
  }'
```

### Example Responses

**200 OK**

```json
{
  "status": "success",
  "response": "projects/create_app/messages/837263829373..."
}
```

**400 Bad Request**

```json
{ "error": "provide exactly one of topic, token or condition" }
```

**401 / 403** — missing/invalid credential, or the user is not an admin for `app`.

**404 Not Found**

```json
{ "error": "App 'create_app' not found in credentials folder." }
```

**503 Service Unavailable** — signing keys or the Firestore admin lookup are unavailable.

### Validation limits (all API routes)

| Input | Rule |
|---|---|
| Request body | max **64 KB** (larger requests get `413`) |
| `topic` (send/save) | `^[A-Za-z0-9\-_.~%]{1,200}$` |
| `condition` | max 500 chars; letters, digits, space and `_ ' " ( ) | & ! . ~ - %` only; balanced quotes; max 5 topics |
| `title` | max 200 characters, non-empty |
| `body` | max 4000 characters |
| `image` | must start with `https://`, max 2048 characters |
| `data` | max 4096 bytes when serialized |
| `tokens` (subscribe) | 1–10 entries, each ≤ 4096 chars, no whitespace |

Invalid input returns `400 Bad Request` with a field-specific message.

### History redaction

The service does **not** store device registration tokens or authorization headers in
`notifications_history`. For token sends, `target` is stored as `"[device-token]"` and
the `token` field is stored as `"[redacted]"`. The authenticated sender is recorded in
`sender_uid` and `sender_email` (`service` for the service key, `legacy` for the legacy key).

---

## 2. List Available Apps

**Endpoint:** `GET /api/v1/apps`
**Authorization:** ID token for a registered project, or `SERVICE_API_KEY`

Returns only the apps where the caller is an admin. The service key returns all loaded apps.

**200 OK**

```json
["create_app", "driver_app", "store_app"]
```

---

## 3. Get Notification History

**Endpoint:** `GET /api/v1/history`
**Authorization:** Admin ID token for `app`, or `SERVICE_API_KEY`

### Query Parameters

| Parameter | Type | Required | Description |
|---|---|---|---|
| `app` | String | No | Filter by app. The caller must be an admin of that app. |
| `topic` | String | No | Filter by target substring. |
| `search` | String | No | Search in `title` or `payload`. |
| `limit` | Integer | No | 1–1000 (default `100`). |

When `app` is omitted, an ID-token caller only sees apps where they are an admin.
The service key sees all apps.

**Example**

```
GET /api/v1/history?app=create_app&limit=50
```

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

## 4. Manage Topics

**Endpoints:** `GET /api/v1/topics`, `POST /api/v1/topics`
**Authorization:** Admin ID token for `app`, or `SERVICE_API_KEY`

### GET

Optional `?app=<name>` filter. Without it, results are limited to apps where the
caller is an admin (the service key sees all).

### POST

```json
{
  "app": "create_app",
  "name": "promo_users"
}
```

`name` follows the topic regex above. Response:

```json
{ "status": "success" }
```

---

## 5. Subscribe to Topic

**Endpoint:** `POST /api/v1/topics/subscribe`

```json
{
  "app": "create_app",
  "topic": "all",
  "tokens": ["device_token_1", "device_token_2"]
}
```

- `app` is **required** and must exist in `credentials/`.
- `topic` is **required** and follows the topic regex.
- `tokens` is **required**: 1–10 tokens, each ≤ 4096 chars, no whitespace.
- Authorization follows the topic policy above. A missing/invalid header is treated
  as anonymous; `users`/`companies`/`technicals` require a valid ID token and
  `admins`/custom topics require an admin.

**200 OK**

```json
{
  "success_count": 2,
  "failure_count": 0,
  "errors": []
}
```

**403 Forbidden** — topic not allowed for the caller's access level.

---

## 6. Unsubscribe from Topic

**Endpoint:** `POST /api/v1/topics/unsubscribe`

Same payload, limits and authorization as subscribe.

---

## Rate limits

| Scope | Limit |
|---|---|
| `/topics/*` per client IP | 30 requests / minute |
| `/topics/*` per authenticated `uid` | 30 requests / minute |
| Failed authentication per client IP | 10 attempts / minute |

Exceeding a limit returns `429 Too Many Requests`.

The client IP is the TCP peer address. `X-Forwarded-For` is used **only** when
`TRUST_PROXY=true`; never enable it unless the service is behind a trusted proxy.

---

## Environment variables

| Variable | Required | Default | Description |
|---|---|---|---|
| `CORS_ALLOWED_ORIGINS` | **Yes** | — | Comma-separated explicit origins. Empty, invalid or `*` prevents startup. |
| `SERVICE_API_KEY` | No | — | Server-to-server key. When unset, service-key auth is disabled. Generate outside the repository. |
| `LEGACY_API_KEY_ENABLED` | No | `false` | Enables the old `API_KEY` on `/send` only. |
| `API_KEY` | No | — | Legacy key. Ignored unless `LEGACY_API_KEY_ENABLED=true`. |
| `TRUST_PROXY` | No | `false` | Trust `X-Forwarded-For` for client IP resolution. |
| `PORT` | No | `8080` | Listen port. |
| `DATABASE_URL` | No | `sqlite://data.db` | SQLite database URL. |

The dashboard email/password variables (`DASHBOARD_EMAIL`, `DASHBOARD_PASSWORD`)
and the `/auth/login` endpoint have been removed.

### Dashboard authentication

The dashboard uses the Firebase Web SDK:

- Build-time variables: `VITE_FIREBASE_CONFIGS` (JSON map of app name to public
  Firebase Web App config) and `VITE_DEFAULT_APP` (project used for sign-in).
  See [dashboard/README.md](dashboard/README.md) and
  [dashboard/.env.example](dashboard/.env.example).
- Every API request sends `Authorization: Bearer <Firebase ID token>`. The token
  is refreshed by the SDK; application code does not store it in
  `localStorage`.
- On `401`, the client force-refreshes once and retries, then returns to
  `/login`.
- On `403`, it shows `هذا الحساب ليس أدمن لهذا التطبيق` without signing out.
- On `503`, it shows `تعذر التحقق من الصلاحية، حاول لاحقاً` without signing out.
- `/apps` returns only apps where the signed-in user is an admin. If exactly one
  app is returned, the dashboard selects it automatically.
- The Firebase Web App configuration is public and is embedded at build time.
  Never place a service-account private key or `SERVICE_API_KEY` in the dashboard.
- If the dashboard is served from a different origin than the API, add its exact
  origin to `CORS_ALLOWED_ORIGINS`. Rebuild `dist/` after changing `VITE_*`
  values; the root `dist/` directory is tracked in git.

---

## Error responses

| Status | Meaning |
|---|---|
| `400` | Invalid request payload or query parameters |
| `401` | Missing, malformed, expired or unverifiable ID token / service key |
| `403` | Valid identity without the required admin role or topic access |
| `404` | Unknown `app`, or unknown endpoint |
| `413` | Request body larger than 64 KB |
| `429` | Rate limit exceeded |
| `503` | Firebase signing keys or Firestore admin lookup unavailable (fail closed) |

---

## Deployment plan

1. **Deploy with the legacy key temporarily enabled.**
   Set `LEGACY_API_KEY_ENABLED=true`, keep the current `API_KEY`, and set
   `CORS_ALLOWED_ORIGINS` to the explicit dashboard origin. This keeps `/send`
   working for the old admin client while the new authentication is rolled out.
2. **Update the admin application and deploy the dashboard.**
   Switch the remaining admin client to Firebase Auth ID tokens
   (`Authorization: Bearer <id_token>`). The dashboard already signs in with
   Firebase Auth; build it with the public Firebase Web App configuration and
   `VITE_DEFAULT_APP`, and add its exact origin to `CORS_ALLOWED_ORIGINS` when it
   is not served by the Rust service. Ensure admins have `role: "admin"` in
   Firestore `users/{uid}` and that each app service account can read Firestore.
3. **Disable the legacy path and rotate keys.**
   Set `LEGACY_API_KEY_ENABLED=false`, remove `API_KEY`, and generate new
   `SERVICE_API_KEY` / `API_KEY` values outside any repository or client bundle.
   Keep `SERVICE_API_KEY` only in the server environment.

## Security notes

- The mobile application key previously embedded in the client must be treated as
  leaked and rotated. No shared secret should be shipped to clients.
- Never commit `.env`, service account JSON files or generated keys.
- Device tokens and authorization headers are never written to logs or history.
- The dashboard stores no API key or ID token in application code. Firebase Auth
  manages its own session; only the public Firebase Web App configuration is
  embedded in the built bundle.

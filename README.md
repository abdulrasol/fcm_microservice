# FCM Microservice & Dashboard

A fully integrated, ultra-lightweight Push Notification Management System built in Rust. It serves as both a high-performance REST API for sending Firebase Cloud Messaging (FCM) notifications to multiple projects and a web-based dashboard.

## Features

- **Multi-App Support**: Send notifications to multiple Firebase projects dynamically by dropping their `service-account.json` into the `credentials/` folder.
- **Firebase ID-token authentication**: Admin routes verify Firebase ID tokens (RS256, issuer, audience, expiry, `auth_time`) and read `users/{uid}.role` from Firestore. No client ships a shared secret.
- **Service-to-service key**: Optional `SERVICE_API_KEY` for trusted backends.
- **Single Docker Container**: The Rust server handles both the REST API and serves the static Web Dashboard on a single port.
- **Ultra Lightweight**: Built with `axum` and `tokio`. Minimal RAM and CPU consumption.
- **Integrated SQLite Database**: Automatically tracks notification history (including sender identity) and saved topics.
- **Hot-Reloading Credentials**: Automatically detects new credentials added to the folder without a server restart.
- **Web Dashboard**: An integrated UI to manually send notifications and view history. Firebase sign-in is added in SEC2-B.

## Documentation

- [API Documentation](docs.md): Endpoints, authentication, permissions, validation, rate limits and the deployment plan.
- [Condition Targeting Guide](condition_guide.md): FCM boolean topic expressions (`&&`, `||`, `!`) and the new validation limits.

## Getting Started

1. Clone the repository.
2. Copy `.env.example` to `.env` and fill in your values. `CORS_ALLOWED_ORIGINS` is mandatory; the service refuses to start without it.
3. **Firebase Setup**:
   - Go to the [Firebase Console](https://console.firebase.google.com/).
   - Navigate to **Project Settings** > **Service accounts** and click **Generate new private key**.
   - Place the JSON file inside `credentials/` and rename it to a short name (e.g. `create_app.json`). **The filename (without `.json`) is the `app` identifier in all requests.**
   - Grant the service account `roles/datastore.viewer` (or broader) so the admin role lookup can read Firestore.
   - Make sure admin users have `role: "admin"` in the Firestore `users/{uid}` document.
4. Run the server using Docker or `cargo run`.

## Authentication in one paragraph

Send `Authorization: Bearer <firebase_id_token>` for admin endpoints, where the token's
project contains the requested `app` and the user's Firestore document has
`role: "admin"`. Server-to-server callers may use `Authorization: Bearer <SERVICE_API_KEY>`.
`/topics/subscribe` and `/topics/unsubscribe` allow anonymous access to public topics
(`all`, `country_XX`, `lang_XX`) and require a valid ID token for role topics.
The legacy `API_KEY` is disabled by default and works only on `/send` while
`LEGACY_API_KEY_ENABLED=true`.

See [docs.md](docs.md) for the full permission table.

## Docker Deployment

```bash
docker build -t fcm-microservice .
docker run -d -p 8080:8080 -v $(pwd)/credentials:/app/credentials -v $(pwd)/data.db:/app/data.db --env-file .env fcm-microservice
```

## Deployment plan (short)

1. Deploy with `LEGACY_API_KEY_ENABLED=true` so the existing admin client keeps using `/send`.
2. Update the admin app and dashboard to send Firebase ID tokens (dashboard sign-in lands in SEC2-B).
3. Set `LEGACY_API_KEY_ENABLED=false`, remove the old `API_KEY`, and generate new
   `API_KEY` / `SERVICE_API_KEY` values outside any repository or client bundle.

> Note: The previous email/password dashboard sign-in has been removed. The
> pre-SEC2-B dashboard login screen will not work until it is migrated to Firebase
> Authentication.

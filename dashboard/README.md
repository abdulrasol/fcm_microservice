# FCM Dashboard

Vue 3 + Vite dashboard for the FCM microservice. It signs users in with the
Firebase Web SDK, sends the resulting ID token to the Rust API, and only shows
apps where the signed-in user is an admin.

## Requirements

- Node.js 22.12+ (Vite 8 requirement)
- A Firebase project with **Email/Password** sign-in enabled
- The backend app service account, used by the API for Firestore admin checks

## Setup

```bash
cd dashboard
pnpm install --ignore-scripts   # pnpm-lock.yaml is the tracked lockfile
cp .env.example .env.local
```

Edit `dashboard/.env.local`:

```dotenv
VITE_FIREBASE_CONFIGS='{"create_app":{"apiKey":"...","authDomain":"...","projectId":"...","appId":"..."}}'
VITE_DEFAULT_APP=create_app
```

`VITE_FIREBASE_CONFIGS` is a JSON object keyed by the backend `app` name (the
service-account filename in `../credentials` without `.json`). Each value is the
**public** Firebase Web App configuration from Firebase Console > Project
Settings > Your apps. These identifiers are embedded in the browser bundle and
are not secrets. Never put a service-account private key or `SERVICE_API_KEY`
here.

`VITE_DEFAULT_APP` selects the Firebase project used for sign-in. If it is
omitted, the first key in the map is used. `/apps` returns only apps where the
signed-in user is an admin in that project.

## Firebase requirements

1. Enable **Email/Password** under Firebase Authentication > Sign-in method.
2. Create a Firestore document `users/{uid}` with `role: "admin"` for every
   dashboard administrator.
3. Add the dashboard's browser origin under Firebase Authentication >
   Settings > Authorized domains.
4. Google sign-in is intentionally left disabled until the Google provider is
   confirmed as enabled. The button is a placeholder with a `TODO(SEC2-B)`
   comment.

## Backend CORS

The backend requires `CORS_ALLOWED_ORIGINS` and refuses to start when it is
empty. If the dashboard is served from a different origin than the API, add the
dashboard origin explicitly, for example:

```dotenv
CORS_ALLOWED_ORIGINS=https://fcm.example.com,http://localhost:5173
```

Do not use `*`. When the Rust service serves the built `dist/` itself, browser
requests are same-origin, but `CORS_ALLOWED_ORIGINS` is still mandatory.

## API behavior

- Every API request sends `Authorization: Bearer <Firebase ID token>`.
- The selected app is added to app-scoped endpoints (`/send`, `/history`,
  `/topics`). The token is refreshed automatically by the SDK.
- On `401`, the client force-refreshes the token once, retries, then returns to
  `/login`.
- On `403`, the dashboard shows `هذا الحساب ليس أدمن لهذا التطبيق` without
  signing the user out.
- On `503`, it shows `تعذر التحقق من الصلاحية، حاول لاحقاً` without signing the
  user out.
- If `/apps` returns exactly one app, it is selected automatically. With more
  than one app, the user selects one from the dropdown.
- No token is written to `localStorage` by application code. Firebase Auth
  persists its own session.

## Build and deployment

```bash
npm run build
```

Vite writes the build to the repository-level `dist/` directory (configured in
`vite.config.js`). The root `dist/` is tracked in git and is served by the Rust
binary; rebuild it before committing dashboard changes. `dashboard/.gitignore`
only ignores `dashboard/dist`, not the root `dist/`.

`VITE_*` values are compiled into the bundle at build time. Rebuild the
dashboard whenever the Firebase configuration or default app changes. The build
contains no service-account key or server API key.

## Manual checks

```bash
npm run build
npm run preview -- --host 127.0.0.1 --port 4173
curl -I http://127.0.0.1:4173/
curl -s http://127.0.0.1:4173/ | grep -o '/assets/[^"]*'
```

Expected: the preview serves `index.html` and the built JS/CSS assets.
With valid Firebase settings, opening `/` without a session redirects to
`/login`; signing in with an admin account redirects back to the dashboard.

What cannot be verified in this repository without real credentials:

- Successful email/password sign-in and password-reset delivery.
- A real `200` from `/apps` and `/send` with an admin ID token.
- The `401` refresh-retry and the `403`/`503` messages against a live backend.
- Google sign-in, because the provider is intentionally not enabled.

Automated checks performed: `npm run build` succeeds, the generated bundle
contains no `/auth/login` endpoint or `fcm_api_key` storage key, and `dist/` is
regenerated from the current source.

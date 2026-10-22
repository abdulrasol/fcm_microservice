# Resolver API example

`resolver_fastapi.py` shows the smallest backend that implements the `http`
data source contract. Any stack can implement it; only the two routes and the
signature matter.

## Contract

The tool sends:

```text
GET {base_url}/doc?path=<percent-encoded full path>
GET {base_url}/members?collection=<c>&field=<f>&value=<v>&limit=<n>
```

- `doc` answers `200` with a JSON object (the document) or `404` when it does
  not exist.
- `members` answers `200 {"uids": ["...", "..."]}`. A `404` here is an error
  for the tool, not an empty list.
- The query parameter order is fixed: `path`; then `collection`, `field`,
  `value`, `limit`.
- Values are percent-encoded with uppercase hex; a document path is encoded as
  one value, so `store_orders/abc123` becomes `store_orders%2Fabc123`.

Every request has two headers:

- `X-FCM-Timestamp`: unix seconds.
- `X-FCM-Signature`: lowercase hex `HMAC-SHA256(secret, canonical)` where

```text
canonical = "GET" + "\n" + <URL path as sent> + "\n" + <raw query as sent> + "\n" + <timestamp>
```

The secret comes from the environment variable named by `secret_env` in the
tool configuration (at least 32 bytes). It is never sent in a request; there is
no `Authorization` header. Reject requests whose timestamp differs from your
clock by more than 60 seconds.

## Test vector (must match the Rust tests)

```text
secret    = test-secret-test-secret-test-secret-00
path      = /internal/fcm/doc
query     = path=store_orders%2Fabc123
timestamp = 1700000000
canonical = GET\n/internal/fcm/doc\npath=store_orders%2Fabc123\n1700000000
signature = f97974c3e1b1706c53e625f9f6b324adab12a0143af3348757cc311771f20813
```

## Run

```bash
export FCM_RESOLVER_SECRET='replace-with-at-least-32-bytes'
python3 examples/resolver_fastapi.py            # serves on 127.0.0.1:8099 (uses FastAPI/uvicorn if available, or falls back to standard library)
```

Optional FastAPI installation:
```bash
pip install fastapi uvicorn
```

Self-test of the signature (standard library only):

```bash
python3 examples/resolver_fastapi.py --selftest  # prints OK
```

## Reverse proxy warning

Sign the path exactly as it is sent. If a reverse proxy rewrites the path
prefix (for example `/internal/fcm/doc` to `/fcm/doc`) or the raw query string,
the signature will not match. Configure the proxy to preserve both.

"""Minimal Resolver API example for fcm_microservice.

The tool calls two signed endpoints:

    GET /internal/fcm/doc?path=<percent-encoded path>
    GET /internal/fcm/members?collection=<c>&field=<f>&value=<v>&limit=<n>

Every request carries `X-FCM-Timestamp` (unix seconds) and `X-FCM-Signature`,
the lowercase hex HMAC-SHA256 of:

    "GET" + "\n" + <path as sent> + "\n" + <raw query as sent> + "\n" + <timestamp>

Only the standard library is needed for `--selftest`; FastAPI is imported when
the example is actually served.
"""

import hashlib
import hmac
import os
import sys
import time

TIMESTAMP_HEADER = "x-fcm-timestamp"
SIGNATURE_HEADER = "x-fcm-signature"
MAX_SKEW_SECONDS = 60

# Test vector shared with the Rust tests and README.md. Do not change it.
TEST_SECRET = b"test-secret-test-secret-test-secret-00"
TEST_PATH = "/internal/fcm/doc"
TEST_QUERY = "path=store_orders%2Fabc123"
TEST_TIMESTAMP = 1700000000
TEST_SIGNATURE = "f97974c3e1b1706c53e625f9f6b324adab12a0143af3348757cc311771f20813"


def canonical(method, path, query, timestamp):
    return "\n".join([method, path, query, str(timestamp)])


def sign(secret, method, path, query, timestamp):
    message = canonical(method, path, query, timestamp).encode("utf-8")
    return hmac.new(secret, message, hashlib.sha256).hexdigest()


def verify(secret, method, path, query, timestamp_header, signature):
    if not timestamp_header or not signature:
        return False
    try:
        timestamp = int(timestamp_header)
    except ValueError:
        return False
    if abs(time.time() - timestamp) > MAX_SKEW_SECONDS:
        return False
    expected = sign(secret, method, path, query, timestamp)
    return hmac.compare_digest(expected, signature)


# Demo data only: replace both lookups with your real database queries.
DOCUMENTS = {
    "store_orders/abc123": {"buyer_id": "u1", "seller_id": "u2", "order_number": 42},
}
USERS = [
    {"uid": "u1", "role": "admin"},
    {"uid": "u2", "role": "member"},
]


def resolve_doc(path):
    return DOCUMENTS.get(path)


def resolve_members(collection, field, value, limit):
    if collection != "users" or field != "role":
        return []
    return [user["uid"] for user in USERS if user["role"] == value][:limit]


def selftest():
    computed = sign(TEST_SECRET, "GET", TEST_PATH, TEST_QUERY, TEST_TIMESTAMP)
    if not hmac.compare_digest(computed, TEST_SIGNATURE):
        print(f"FAIL: expected {TEST_SIGNATURE}, got {computed}")
        return 1
    print("OK")
    return 0


def create_app():
    # Imported here so `--selftest` runs without FastAPI installed.
    from fastapi import FastAPI, Request
    from fastapi.responses import JSONResponse

    app = FastAPI()
    secret = os.environ["FCM_RESOLVER_SECRET"].encode("utf-8")

    def authorized(request):
        return verify(
            secret,
            request.method,
            request.url.path,
            request.url.query,  # raw query string, exactly as sent
            request.headers.get(TIMESTAMP_HEADER),
            request.headers.get(SIGNATURE_HEADER),
        )

    @app.get("/internal/fcm/doc")
    def doc(request: Request, path: str):
        if not authorized(request):
            return JSONResponse({"error": "unauthorized"}, status_code=401)
        document = resolve_doc(path)
        if document is None:
            return JSONResponse({"error": "not found"}, status_code=404)
        return document

    @app.get("/internal/fcm/members")
    def members(
        request: Request, collection: str, field: str, value: str, limit: int = 100
    ):
        if not authorized(request):
            return JSONResponse({"error": "unauthorized"}, status_code=401)
        return {"uids": resolve_members(collection, field, value, limit)}

    return app


if __name__ == "__main__":
    if "--selftest" in sys.argv:
        sys.exit(selftest())
    import uvicorn

    uvicorn.run(create_app(), host="127.0.0.1", port=8099)

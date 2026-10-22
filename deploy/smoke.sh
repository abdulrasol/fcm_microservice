#!/usr/bin/env bash
# Post-deploy checks that need no secret and send no notification.
# Usage: deploy/smoke.sh [base_url]     (default http://127.0.0.1:${HOST_PORT:-8084})
set -uo pipefail

BASE="${1:-http://127.0.0.1:${HOST_PORT:-8084}}"
FAIL=0

check() { # name expected_codes actual
  local name="$1" expected="$2" actual="$3"
  if [[ " $expected " == *" $actual "* ]]; then
    echo "PASS  $name ($actual)"
  else
    echo "FAIL  $name (expected $expected, got $actual)"
    FAIL=1
  fi
}

code() { curl -s -o /dev/null -w '%{http_code}' --max-time 10 "$@" || true; }

check "health"                          "200" "$(code "$BASE/health")"
check "dashboard index is served"       "200" "$(code "$BASE/")"
check "index contains the app root"     "yes" "$(curl -s --max-time 10 "$BASE/" | grep -q 'id="app"' && echo yes || echo no)"
check "admin /auth/me without a session" "401" "$(code "$BASE/api/v1/admin/auth/me")"
check "admin login without CSRF header" "403" "$(code -X POST -H 'Content-Type: application/json' -d '{"email":"x@x.x","password":"x"}' "$BASE/api/v1/admin/auth/login")"
check "send without credentials"        "401 403" "$(code -X POST -H 'Content-Type: application/json' -d '{}' "$BASE/api/v1/send")"
check "apps without credentials"        "401 403" "$(code "$BASE/api/v1/apps")"
FAKE_KEY="fcm_smoke_$(printf 'A%.0s' $(seq 1 43))"
check "unknown app key is rejected"     "401" "$(code -X POST -H "Authorization: Bearer $FAKE_KEY" -H 'Content-Type: application/json' -d '{}' "$BASE/api/v1/send")"
check "unknown api path"                "404" "$(code "$BASE/api/v1/nope")"

if command -v docker >/dev/null 2>&1 && docker ps --format '{{.Names}}' | grep -qx fcm_microservice; then
  PANICS=$(docker logs --tail 500 fcm_microservice 2>&1 | grep -ci 'panicked')
  check "no panic in the last 500 log lines" "0" "$PANICS"
fi

if [ "$FAIL" -ne 0 ]; then
  echo "SMOKE FAILED"
  exit 1
fi
echo "SMOKE OK"

#!/usr/bin/env bash
# Compares the routes of docs/permissions.md with the fixed route table of
# docs/plans/reports/T13A-routes.md.
#
# The route table is the one the Rust test `route_table_is_documented` compares
# against the router built in `src/main.rs`. This script makes sure the human
# document did not drift from it.
#
# Usage: bash scripts/check_permissions_doc.sh
# Exit:  0 = the two documents list exactly the same routes.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROUTES="$ROOT/docs/plans/reports/T13A-routes.md"
DOC="$ROOT/docs/permissions.md"

for file in "$ROUTES" "$DOC"; do
  if [ ! -f "$file" ]; then
    echo "FAIL: the file is missing: $file" >&2
    exit 1
  fi
done

# The fixed table: the lines between the ```routes fence.
extract_routes() {
  awk '
    /^```routes$/ { inside = 1; next }
    /^```$/ && inside { inside = 0; next }
    inside && NF { print }
  ' "$ROUTES" | awk -F'|' '{ gsub(/^[ \t]+|[ \t]+$/, "", $1); gsub(/^[ \t]+|[ \t]+$/, "", $2); print $1 " " $2 }' | sort
}

# The document: every table row whose second cell is a path.
extract_doc() {
  grep -E '^\| [0-9]+ \| (GET|POST|PUT|DELETE|PATCH) \|' "$DOC" \
    | awk -F'|' '{ gsub(/^[ \t]+|[ \t]+$/, "", $3); gsub(/^[ \t]+|[ \t]+$/, "", $4); print $3 " " $4 }' \
    | sed 's/`//g' \
    | sort
}

extract_routes > /tmp/check_permissions_routes.$$
extract_doc > /tmp/check_permissions_doc.$$

status=0

count_routes=$(wc -l < /tmp/check_permissions_routes.$$ | tr -d ' ')
count_doc=$(wc -l < /tmp/check_permissions_doc.$$ | tr -d ' ')

if [ "$count_routes" != "$count_doc" ]; then
  echo "FAIL: the counts differ: T13A-routes.md=$count_routes, permissions.md=$count_doc" >&2
  status=1
fi

if ! diff -u /tmp/check_permissions_routes.$$ /tmp/check_permissions_doc.$$ > /tmp/check_permissions_diff.$$; then
  echo "FAIL: the routes differ (- = T13A-routes.md, + = permissions.md):" >&2
  cat /tmp/check_permissions_diff.$$ >&2
  status=1
fi

rm -f /tmp/check_permissions_routes.$$ /tmp/check_permissions_doc.$$ /tmp/check_permissions_diff.$$

if [ "$status" -eq 0 ]; then
  echo "OK: $count_routes routes match"
fi

exit "$status"

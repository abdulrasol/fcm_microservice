#!/usr/bin/env bash
# Verifies that:
# 1) Every variable in .env.example is documented in docs/deployment.md
# 2) Every environment variable in docs/deployment.md exists in src/ or .env.example.
#
# Usage: bash scripts/check_env_vars.sh

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

export ROOT
python3 - <<'EOF'
import os
import re
import sys

root = os.environ["ROOT"]
env_example_path = os.path.join(root, ".env.example")
deployment_doc_path = os.path.join(root, "docs", "deployment.md")

# 1. Variables from .env.example
env_vars = []
with open(env_example_path, "r", encoding="utf-8") as f:
    for line in f:
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            var_name = line.split("=")[0].strip()
            env_vars.append(var_name)

# 2. Documented variables in docs/deployment.md (from table in section 1)
doc_vars = []
with open(deployment_doc_path, "r", encoding="utf-8") as f:
    content = f.read()

# Match variables enclosed in backticks in markdown table or text: `NAME`
table_section = content.split("## 2.")[0]
raw_doc_matches = re.findall(r'`([A-Z][A-Z0-9_]{2,})`', table_section)
for v in raw_doc_matches:
    if v not in doc_vars and v not in ["TCP", "HTTP", "HTTPS", "SQLite", "WAL", "JSON", "CLI", "POST", "PUT", "DELETE"]:
        doc_vars.append(v)

# Check 1: Is every .env.example variable documented?
missing_in_doc = [v for v in env_vars if v not in doc_vars]
if missing_in_doc:
    print(f"FAIL: The following variables from .env.example are missing in docs/deployment.md: {missing_in_doc}", file=sys.stderr)
    sys.exit(1)

# Check 2: Does every documented variable exist in src/ or .env.example?
# Search in src/
src_text = ""
for dirpath, _, filenames in os.walk(os.path.join(root, "src")):
    for fname in filenames:
        if fname.endswith(".rs"):
            with open(os.path.join(dirpath, fname), "r", encoding="utf-8") as sf:
                src_text += sf.read() + "\n"

# Variables can also be dynamic application secrets like CLIENT_JWT_SECRET / CLIENT_RESOLVER_SECRET
known_dynamic_examples = ["CLIENT_JWT_SECRET", "CLIENT_RESOLVER_SECRET"]

missing_in_src_or_env = []
for v in doc_vars:
    if v not in env_vars and v not in src_text and v not in known_dynamic_examples:
        missing_in_src_or_env.append(v)

if missing_in_src_or_env:
    print(f"FAIL: Documented variables not found in src/ or .env.example: {missing_in_src_or_env}", file=sys.stderr)
    sys.exit(1)

print(f"OK: All {len(env_vars)} variables from .env.example are documented in docs/deployment.md:")
for v in env_vars:
    print(f"  - {v} [documented]")

print(f"\nAll {len(doc_vars)} documented environment variables are verified against src/ and .env.example.")
sys.exit(0)
EOF

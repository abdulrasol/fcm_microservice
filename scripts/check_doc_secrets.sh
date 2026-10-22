#!/usr/bin/env bash
# Verifies that no private keys, real API keys, or leaked app key patterns
# exist in the public documentation and examples.
#
# Checks files: README.md, docs.md, curl_api.md, .agent.md, docs/** (excluding docs/plans/), examples/**
#
# Usage: bash scripts/check_doc_secrets.sh
# Exit: 0 = no secret leaks found.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

export ROOT
python3 - <<'EOF'
import os
import re
import sys

root = os.environ["ROOT"]

files_to_check = [
    os.path.join(root, "README.md"),
    os.path.join(root, "docs.md"),
    os.path.join(root, "curl_api.md"),
    os.path.join(root, ".agent.md"),
]

for dirpath, _, filenames in os.walk(os.path.join(root, "docs")):
    # Exclude docs/plans/ entirely (contains historical reports and execution prompts)
    if os.path.relpath(dirpath, root).startswith("docs/plans"):
        continue
    for f in filenames:
        if f.endswith(".md"):
            files_to_check.append(os.path.join(dirpath, f))

for dirpath, _, filenames in os.walk(os.path.join(root, "examples")):
    for f in filenames:
        files_to_check.append(os.path.join(dirpath, f))

# Patterns to detect
p_privkey = re.compile(r'BEGIN\s+(?:RSA\s+|EC\s+)?PRIVATE\s+KEY|-----BEGIN')
p_aiza = re.compile(r'AIza[0-9A-Za-z_-]{20,}')
p_appkey = re.compile(r'fcm_[a-z0-9_-]+_[A-Za-z0-9_-]{16,}')

violations = []

for filepath in files_to_check:
    if not os.path.exists(filepath):
        continue
    rel = os.path.relpath(filepath, root)
    with open(filepath, "r", encoding="utf-8", errors="ignore") as f:
        for idx, line in enumerate(f, 1):
            # Check private key
            if p_privkey.search(line):
                violations.append((rel, idx, "Private key header pattern", line.strip()))
            # Check Google API key
            if p_aiza.search(line):
                violations.append((rel, idx, "Google API key pattern (AIza...)", line.strip()))
            # Check app key string
            for m in p_appkey.finditer(line):
                matched = m.group(0)
                # Allow only explicit placeholder syntax like <43-char-random>
                if "<" in line and ">" in line:
                    continue
                violations.append((rel, idx, f"App key pattern ({matched})", line.strip()))

if violations:
    print(f"FAILED: Found {len(violations)} secret leak(s):", file=sys.stderr)
    for path, line_no, reason, text in violations:
        print(f"  {path}:{line_no} [{reason}]: {text}", file=sys.stderr)
    sys.exit(1)

print(f"OK: Scanned {len(files_to_check)} documentation and example files; 0 secrets found.")
sys.exit(0)
EOF

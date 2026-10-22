#!/usr/bin/env bash
# Verifies that every relative markdown link in README.md and docs/** points to an existing file.
#
# Usage: bash scripts/check_internal_links.sh
# Exit: 0 = all relative links point to existing files.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

python3 - <<EOF
import os
import re
import sys

root = "$ROOT"
files_to_check = [os.path.join(root, "README.md")]

for dirpath, _, filenames in os.walk(os.path.join(root, "docs")):
    for f in filenames:
        if f.endswith(".md"):
            files_to_check.append(os.path.join(dirpath, f))

# Regex for Markdown links: [text](target)
# Excludes http://, https://, mailto:, and empty links
link_re = re.compile(r'\[([^\]]+)\]\(([^)]+)\)')

errors = 0
total_links = 0

for filepath in files_to_check:
    rel_source = os.path.relpath(filepath, root)
    with open(filepath, "r", encoding="utf-8") as f:
        content = f.read()

    # Find code blocks to ignore links inside code blocks
    for match in link_re.finditer(content):
        target = match.group(2).strip()

        # Skip external or anchor-only links
        if target.startswith("http://") or target.startswith("https://") or target.startswith("mailto:") or target.startswith("#"):
            continue

        # Strip fragment identifier (e.g., file.md#section -> file.md)
        clean_target = target.split("#")[0]
        if not clean_target:
            continue

        total_links += 1
        source_dir = os.path.dirname(filepath)
        resolved_path = os.path.normpath(os.path.join(source_dir, clean_target))

        if not os.path.exists(resolved_path):
            print(f"BROKEN LINK in {rel_source}: '{target}' -> resolved as '{os.path.relpath(resolved_path, root)}' (not found)", file=sys.stderr)
            errors += 1

if errors > 0:
    print(f"\nFAILED: {errors} broken internal link(s) found.", file=sys.stderr)
    sys.exit(1)
else:
    print(f"OK: verified {total_links} internal relative links across {len(files_to_check)} files.")
    sys.exit(0)
EOF

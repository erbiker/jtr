#!/usr/bin/env bash
# Recompute SHA-256 checksums for every manifest referenced from jtr-index/index.json
# and rewrite the `sha256` field on each entry in place.
#
# Run this after editing any recipe manifest, then commit the index.json change.
# Requires `python3` (stdlib only, no extra deps).
set -euo pipefail

cd "$(dirname "$0")/.."

python3 <<'PY'
import hashlib
import json
import sys
from pathlib import Path

INDEX = Path("jtr-index/index.json")
data = json.loads(INDEX.read_text())

updated = 0
for entry in data["recipes"]:
    manifest_url = entry["manifest_url"]
    if "://" in manifest_url:
        sys.exit(
            f"refusing to hash absolute URL '{manifest_url}' for '{entry['name']}'; "
            "this script only handles relative manifest paths."
        )
    manifest_path = INDEX.parent / manifest_url
    digest = hashlib.sha256(manifest_path.read_bytes()).hexdigest()
    if entry.get("sha256") != digest:
        updated += 1
    entry["sha256"] = digest


def dump(obj, level=0, indent=2):
    pad = " " * (indent * level)
    inner = " " * (indent * (level + 1))
    if isinstance(obj, dict):
        if not obj:
            return "{}"
        items = [
            f'{inner}{json.dumps(k)}: {dump(v, level + 1, indent)}'
            for k, v in obj.items()
        ]
        return "{\n" + ",\n".join(items) + "\n" + pad + "}"
    if isinstance(obj, list):
        if not obj:
            return "[]"
        if all(isinstance(x, (str, int, float, bool)) or x is None for x in obj):
            return "[" + ", ".join(json.dumps(x) for x in obj) + "]"
        items = [inner + dump(x, level + 1, indent) for x in obj]
        return "[\n" + ",\n".join(items) + "\n" + pad + "]"
    return json.dumps(obj)


INDEX.write_text(dump(data) + "\n")
print(f"rehashed {len(data['recipes'])} recipes ({updated} changed) in {INDEX}")
PY

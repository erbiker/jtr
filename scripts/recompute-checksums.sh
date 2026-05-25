#!/usr/bin/env bash
# Recompute SHA-256 checksums for every manifest referenced from
# jtr-index/index.json and rewrite the `sha256` field on each entry in place.
#
# Thin wrapper around `jtr lint --tap jtr-index --fix` — kept for back-compat
# so existing muscle memory (`just rehash`) still works. New tap maintainers
# should run `jtr lint --tap <their-tap> --fix` directly.
set -euo pipefail

cd "$(dirname "$0")/.."

if [ -x target/release/jtr ]; then
    BIN=target/release/jtr
elif [ -x target/debug/jtr ]; then
    BIN=target/debug/jtr
else
    cargo build >/dev/null
    BIN=target/debug/jtr
fi

exec "$BIN" lint --tap jtr-index --fix

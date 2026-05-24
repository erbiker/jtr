# jtr's own justfile. Dogfood: this is what we want jtr to feel like.
#
# Run `just` (no args) to see the full list of recipes.

default:
    @just --list

# Compile the debug binary.
build:
    cargo build

# Compile the release binary with full optimizations.
build-release:
    cargo build --release

# Run all tests (unit + integration).
test:
    cargo test

# Auto-format the workspace.
fmt:
    cargo fmt --all

# Verify formatting without modifying files (used by CI).
fmt-check:
    cargo fmt --all -- --check

# Lint with clippy; warnings fail the build.
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# The full quality gate. Sessions must run this before declaring done.
check: fmt-check lint test

# End-to-end smoke test against the bundled sample index in a throwaway tempdir.
smoke: build
    #!/usr/bin/env bash
    set -euo pipefail
    BIN="$PWD/target/debug/jtr"
    INDEX="file://$PWD/jtr-index/index.json"
    DEMO=$(mktemp -d)
    trap "rm -rf $DEMO" EXIT
    cd "$DEMO"
    printf 'default:\n    @echo hi\n' > justfile
    JTR_INDEX_URL="$INDEX" "$BIN" install postgres-dev
    JTR_INDEX_URL="$INDEX" "$BIN" install rust-lint-format
    JTR_INDEX_URL="$INDEX" "$BIN" list
    just --justfile justfile --list >/dev/null
    JTR_INDEX_URL="$INDEX" "$BIN" remove postgres-dev
    JTR_INDEX_URL="$INDEX" "$BIN" list
    echo "smoke test ok"

# Remove build artifacts.
clean:
    cargo clean

# Recompute sha256 checksums for every manifest referenced from jtr-index/index.json.
# Run after editing any recipe manifest, then commit the resulting index.json change.
rehash:
    scripts/recompute-checksums.sh

# Validate that every recipe in jtr-index/ parses as valid `just` syntax.
# Installs each recipe individually into a temp justfile and runs `just --list`
# to ensure the registry's seed catalog cannot ship a recipe that breaks user files.
validate-index: build
    #!/usr/bin/env bash
    set -euo pipefail
    BIN="$PWD/target/debug/jtr"
    INDEX="file://$PWD/jtr-index/index.json"
    failed=0
    for recipe in $(JTR_INDEX_URL="$INDEX" "$BIN" search 2>/dev/null | awk '{print $1}'); do
        DEMO=$(mktemp -d)
        printf 'default:\n    @echo hi\n' > "$DEMO/justfile"
        if JTR_INDEX_URL="$INDEX" "$BIN" --file "$DEMO/justfile" install "$recipe" >/dev/null 2>&1; then
            if ! just --justfile "$DEMO/justfile" --list >/dev/null 2>&1; then
                echo "FAIL: $recipe produced an unparseable justfile"
                failed=1
            else
                echo "ok: $recipe"
            fi
        fi
        rm -rf "$DEMO"
    done
    if [ "$failed" -ne 0 ]; then
        echo ""
        echo "one or more recipes are broken"
        exit 1
    fi

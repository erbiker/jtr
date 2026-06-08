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

# Verify TOML files (Cargo.toml, deny.toml) are taplo-formatted.
taplo-check:
    taplo fmt --check

# Auto-format every TOML file via taplo.
taplo-fmt:
    taplo fmt

# Detect unused dependencies in Cargo.toml.
machete:
    cargo machete

# Audit dependencies: licenses, security advisories, banned crates, duplicate
# versions, unknown registries. Requires network (fetches RustSec advisories).
deny:
    cargo deny check

# The full quality gate. Sessions must run this before declaring done.
check: fmt-check lint taplo-check machete deny test

# End-to-end smoke test against the bundled sample index in a throwaway tempdir.
smoke: build
    #!/usr/bin/env bash
    set -euo pipefail
    BIN="$PWD/target/debug/jtr"
    INDEX="file://$PWD/jtr-index/index.json"
    CURATED="$PWD/jtr-index"
    DEMO=$(mktemp -d)
    trap "rm -rf $DEMO" EXIT
    cd "$DEMO"
    printf 'default:\n    @echo hi\n' > justfile
    JTR_INDEX_URL="$INDEX" "$BIN" install postgres-dev
    JTR_INDEX_URL="$INDEX" "$BIN" install rust-lint-format
    JTR_INDEX_URL="$INDEX" "$BIN" show redis-dev | grep -q '^# >>> jtr:redis-dev@'
    JTR_INDEX_URL="$INDEX" "$BIN" diff rust-lint-format
    JTR_INDEX_URL="$INDEX" "$BIN" info redis-dev | grep -q 'curated'
    JTR_INDEX_URL="$INDEX" "$BIN" info redis-dev --json | grep -q '"source": "curated"'
    # Freshly-installed blocks are current, so --dry-run is a silent exit-0 no-op.
    test -z "$(JTR_INDEX_URL="$INDEX" "$BIN" update --dry-run)"
    JTR_INDEX_URL="$INDEX" "$BIN" list
    just --justfile justfile --list >/dev/null
    JTR_INDEX_URL="$INDEX" "$BIN" remove postgres-dev
    JTR_INDEX_URL="$INDEX" "$BIN" list
    # Lint the curated index — should pass cleanly.
    "$BIN" lint --tap "$CURATED"
    # Scaffold + lint round trip in an isolated tap.
    TAP=$(mktemp -d)
    printf '{\n  "version": 1,\n  "recipes": []\n}\n' > "$TAP/index.json"
    (cd "$TAP" && "$BIN" scaffold recipe demo-recipe)
    "$BIN" lint --tap "$TAP" --fix
    rm -rf "$TAP"
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

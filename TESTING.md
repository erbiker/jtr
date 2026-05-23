# Testing discipline

## The regression rule

**For every behavior change, write at least one test that would have failed before the change and passes after it.** No exceptions for the CLI's behavior, the manifest format, the managed-block layout, or the index fetching.

If you find yourself unable to write that test, the change is probably wrong-shaped — too coupled, too implicit, or doing the wrong thing. Stop and reconsider before forcing a test in.

## Three tiers

1. **Unit tests** — pure-logic functions: `managed::*`, `target::classify`, manifest deserialization. Live in `#[cfg(test)] mod tests` blocks in the same file as the code. Fast, isolated, deterministic.

2. **Integration tests** — the compiled binary, invoked via `assert_cmd`, with a real (temp) project directory and a `file://` sample index. Live in `tests/cli.rs`. These catch regressions in CLI wiring, argument parsing, and end-to-end flows.

3. **Smoke test** — `just smoke` runs an install/list/remove loop end-to-end against the bundled sample index. Run manually before opening a PR. Not a substitute for an integration test of the same flow — both should exist.

## What to cover

### Always cover
- Every new `jtr` subcommand: a happy-path integration test.
- Every error path you can imagine a user hitting: recipe-not-found, missing project file, malformed index, version mismatch.
- Every conditional branch in `managed.rs` — string surgery on user files is where bugs hide.

### Cover proportionally
- Pure data types (manifest deserialization): one round-trip test per type is fine.
- Internal-only helpers: only if their logic is non-trivial.

### Do not cover
- The third-party crates we depend on (`reqwest`, `clap`, `serde_json`).
- Trivial getter-style code.

## Running the suite

```sh
just test         # cargo test (unit + integration)
just smoke        # end-to-end against bundled sample index
just check        # fmt + clippy + test, the full quality gate
```

## Adding an integration test (the pattern)

Use `assert_cmd` + `tempfile`. The pattern looks like:

```rust
use assert_cmd::Command;
use tempfile::TempDir;
use std::fs;

#[test]
fn install_appends_managed_block() {
    let project = TempDir::new().unwrap();
    fs::write(project.path().join("justfile"), "default:\n    @echo hi\n").unwrap();

    let index_url = format!(
        "file://{}/jtr-index/index.json",
        env!("CARGO_MANIFEST_DIR")
    );

    Command::cargo_bin("jtr")
        .unwrap()
        .current_dir(project.path())
        .env("JTR_INDEX_URL", &index_url)
        .args(["install", "postgres-dev"])
        .assert()
        .success();

    let result = fs::read_to_string(project.path().join("justfile")).unwrap();
    assert!(result.contains("# >>> jtr:postgres-dev@0.1.0 >>>"));
    assert!(result.contains("# <<< jtr:postgres-dev <<<"));
}
```

See [tests/cli.rs](tests/cli.rs) for the live examples.

## What good test names look like

- `install_appends_managed_block` ✅ describes the behavior under test
- `install_existing_recipe_is_idempotent` ✅
- `remove_collapses_surrounding_blank_lines` ✅
- `test_install` ❌ vague
- `it_works` ❌ uninformative

## When fixing a bug

Two-step ritual:

1. Write a test that reproduces the bug (it should fail).
2. Fix the bug. The test should pass.

Commit both together. The test name describes the bug.

## When refactoring (no behavior change)

The test suite must pass before and after, with no new tests required. If a refactor breaks tests, either the refactor changed behavior (write a test for the new behavior, document the change in CHANGELOG) or the test was over-coupled to internals (rewrite the test against the public behavior, not the private structure).

## CI

`.github/workflows/ci.yml` runs `cargo fmt --check`, `cargo clippy -- -D warnings`, and `cargo test` on every push and PR. **Do not merge red CI.** If CI catches a class of bug that local `just check` did not, add the same check to `just check`.

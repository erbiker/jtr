# Changelog

All notable changes to this project are documented here.

This project follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

_Entries land here as changes merge; cut a release tag to roll them into a numbered version._

### Added
- **Checksum verification on fetch.** `IndexEntry` gains an optional `sha256` field; `Registry::load_manifest` now hashes the fetched manifest bytes and refuses to return a manifest whose hash doesn't match what the index declared. Errors include both the expected and actual hashes plus the source URL. If an index entry omits `sha256`, install/update succeeds with a `warning:` printed to stderr — backwards-compatible during the v1 rollout but visible enough that maintainers notice and add the hash. All 5 seed recipes in `jtr-index/index.json` now ship with checksums.
- `scripts/recompute-checksums.sh` (also wired up as `just rehash`) — Python-only, no external deps. Walks `jtr-index/index.json`, recomputes the SHA-256 of every referenced manifest, and rewrites the file in place. Idempotent: a no-op run produces a zero-line diff.
- `jtr update [<name>]` — re-fetch one or all installed recipes and replace their managed block when the registry has a newer version. Reports `updated X → Y` for version bumps, `refreshed @X` when the block had drifted from canonical, and `already at version X` when nothing changed. Wires through to the existing `Registry` and `managed::upsert` pipeline so the same atomicity/idempotency guarantees apply.
- Project [`justfile`](justfile) with `just check` (fmt + clippy + test), `just smoke` (end-to-end loop), and `just validate-index` (every seed recipe parses as valid `just` syntax).
- 16 end-to-end integration tests in [`tests/cli.rs`](tests/cli.rs) exercising the compiled binary against the bundled sample index. Dev-deps `assert_cmd`, `predicates`, and `tempfile`.
- GitHub Actions CI workflow ([.github/workflows/ci.yml](.github/workflows/ci.yml)) running fmt + clippy + tests + smoke + validate-index on Linux and macOS.
- Open-source repository hardening:
  - Dual [MIT](LICENSE-MIT) / [Apache-2.0](LICENSE-APACHE) license files (matching the `Cargo.toml` declaration and the Rust ecosystem convention).
  - [CONTRIBUTING.md](CONTRIBUTING.md) — development setup, PR workflow, and recipe-authoring guide.
  - [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) — based on Contributor Covenant v2.1.
  - [SECURITY.md](SECURITY.md) — private vulnerability-disclosure process.
  - GitHub issue templates (bug, feature, recipe proposal) and a pull request template under `.github/`.

### Documentation
- README and CONTRIBUTING.md now note the planned pre-v1.0 split of the recipe registry into its own repository (Homebrew tap model). Manifest format won't change; only the file location and the PR target will.

## [0.1.0] — 2026-05-23

The initial scaffold. Pre-alpha; APIs and recipe formats will change.

### Added
- `jtr install <name>` — fetches a recipe manifest from the registry index and writes a sentinel-delimited managed block into the project's `justfile`.
- `jtr remove <name>` — strips a previously-installed managed block, collapsing surrounding blank lines so the file stays tidy across install/remove cycles.
- `jtr list` (alias `jtr ls`) — lists the names and versions of all jtr-managed recipes in the current project file.
- `jtr search [query]` — case-insensitive substring search across recipe names and descriptions in the registry index.
- Registry index loader supporting `https://`, `http://`, `file://`, and bare local-path URLs (configurable via `--index` or `JTR_INDEX_URL`).
- Auto-detection of `justfile` / `Justfile` / `Taskfile.yml` / `Taskfile.yaml` in the current directory; `--file` override.
- `task` (YAML) target stub — install detects Taskfile.yml and exits with a friendly "not yet implemented" message including the would-be snippet.
- Bundled sample registry under `jtr-index/` with 5 seed recipes: `postgres-dev`, `redis-dev`, `rust-lint-format`, `node-lint-format`, `clean`. All recipes use language prefixes (`rust-lint`, `node-lint`, etc.) so installing multiple language formatters together doesn't collide.
- Unit test suite for `managed.rs` (render, parse, upsert, remove, validate-name).
- [README.md](README.md) — user-facing intro.

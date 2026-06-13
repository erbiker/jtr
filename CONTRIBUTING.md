# Contributing to jtr

Thanks for thinking about contributing! `jtr` is a small, focused tool and the codebase is approachable — most changes only touch one or two files.

This guide covers the practical setup. For project history and what's shipped, see [CHANGELOG.md](CHANGELOG.md).

## Ways to contribute

There are three paths that make sense for most people:

1. **Contribute a recipe.** The registry lives at `jtr-index/`. Every JSON manifest there is a recipe — add yours, open a PR. This is the easiest, highest-leverage way to help. See [Contributing a recipe](#contributing-a-recipe) below.
2. **Fix a bug or add a feature.** Pick an open issue (look for the `good first issue` label if you're new), or file one to discuss before writing code if the change is non-trivial.
3. **Improve docs.** The README, this guide, and inline command help are all fair game.

If you're not sure what to work on, [GitHub Discussions](https://github.com/erbiker/jtr/discussions) is a good place to ask.

## Development setup

You need:

- **Rust 1.95 or newer** (install via [rustup](https://rustup.rs)).
- **[`just`](https://github.com/casey/just)** — this project's own build commands run through `just`. Install via `brew install just`, `cargo install just`, or your package manager.
- **`cargo-deny`, `cargo-machete`, and `taplo`** — supply-chain and formatting checks that `just check` runs. Install via `brew install cargo-deny taplo && cargo install --locked cargo-machete`, or skip if you only plan to run pieces of `check` individually (CI catches the same things on your PR).

```sh
git clone https://github.com/erbiker/jtr
cd jtr
cargo build
just --list    # see available project commands
```

## The development loop

```sh
just check       # fmt + clippy + taplo + machete + deny + test (the full quality gate)
just smoke       # end-to-end install/list/remove against the bundled sample index
just validate-index   # confirm every recipe in jtr-index/ produces valid just syntax
```

`just check` is what CI runs. If it passes locally, your PR should pass CI. The dep-audit pieces (`cargo deny check` in particular) need network to fetch the RustSec advisory database — if you're offline, the rest of `check` still runs offline-friendly.

When iterating, you can run pieces individually:

```sh
cargo test                                          # unit + integration tests
cargo test --test cli install_appends               # a specific integration test
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all
just taplo-fmt                                      # auto-format Cargo.toml + deny.toml
just deny                                           # licenses + advisories + duplicates
just machete                                        # unused dependency detector
```

## Submitting a pull request

1. **Open an issue first** if the change is non-trivial — it saves you from writing code that won't merge.
2. **Branch from `main`.** Keep PRs focused; one logical change per PR is much easier to review.
3. **Run `just check` locally** before pushing. CI will run it too, but catching failures locally is faster.
4. **Write a clear commit message and PR description.** Explain *why*, not just *what* — the diff already shows what.
5. **Add tests** for behavior changes. See [TESTING.md](TESTING.md) for the patterns we use.
6. **Update [CHANGELOG.md](CHANGELOG.md)** under `## [Unreleased]` for any user-visible change.

A reviewer will respond within a few days. Be patient — this is a side project, not a job.

## Contributing a recipe

A recipe is a JSON manifest in [`jtr-index/recipes/`](jtr-index/recipes/), plus a matching entry in [`jtr-index/index.json`](jtr-index/index.json).

> **Heads up — planned repo split.** Today the recipe registry lives inside this CLI repo for convenience. Before v1.0, we plan to split it out into a separate `<name>-index` repository, following the Homebrew tap model. The goal is that contributing a recipe shouldn't require pulling down or touching CLI code. The manifest format itself won't change in the move — only the file location and the PR URL. If you're contributing a recipe before that split happens, you're contributing here; we'll migrate everything cleanly when the time comes.

### Recipe manifest format

```json
{
  "name": "my-recipe",
  "version": "0.1.0",
  "description": "One-line description shown in `jtr search`",
  "homepage": "https://github.com/erbiker/jtr-index/tree/main/recipes/my-recipe",
  "maintainer": "your-github-handle",
  "shells_out_to": ["docker", "psql"],
  "targets": {
    "just": {
      "snippet": "my-up:\n    @echo hello\n"
    }
  }
}
```

Then add a corresponding entry to `index.json`:

```json
{
  "name": "my-recipe",
  "version": "0.1.0",
  "description": "...",
  "manifest_url": "recipes/my-recipe.json",
  "targets": ["just"]
}
```

After adding or editing **any** manifest, run `just rehash` (or `scripts/recompute-checksums.sh`) to refresh the `sha256` field on every entry in `index.json`. This is mechanical — the script does it for you — but it's required: shipping a manifest whose hash doesn't match the index will cause every install to fail with a checksum error.

### Guidelines for a good recipe

- **Solve a real, frequently-recurring problem.** "Start a Postgres dev container with a healthcheck" — yes. "My personal blog deploy script" — no.
- **Name it predictably.** Lowercase with hyphens. Use a language prefix for language-specific recipes (`rust-lint-format`, `node-lint-format`) so users can install several side-by-side without recipe-name collisions.
- **Be explicit about dependencies.** List everything the recipe shells out to in `shells_out_to`. Users should never be surprised by a missing binary.
- **No `curl | bash` patterns.** No fetching arbitrary code from the internet at runtime. Recipes should be auditable from the manifest alone.
- **Document via comments inside the snippet.** Each recipe in the snippet should have a one-line `#` comment describing what it does, so it shows up in `just --list`.
- **Validate locally:**
  ```sh
  just validate-index
  ```
  This installs every recipe in the bundled index into a throwaway justfile and asserts that `just --list` parses it cleanly. Your new recipe must pass this check.

  If your recipe ships a `task` (go-task) target, also run `jtr lint --tap jtr-index` with `task` installed (`brew install go-task`) — `lint` parses each `task` snippet against the real `task` binary, and skips that check with a warning if `task` isn't on `PATH`. CI installs `task` and runs this automatically, so a broken `task` snippet fails CI; validating locally first saves a round trip.

## Code style

- **Errors:** use `anyhow::Result` with `.with_context(...)` for context. Don't introduce custom error enums unless we need programmatic discrimination.
- **Comments:** explain *why*, not *what*. If the code's intent isn't clear from naming, improve the naming first; reach for a comment only when there's a subtle invariant or workaround.
- **No premature abstractions.** Three similar lines is fine. Pull out a helper when there's a third use.

`cargo clippy -D warnings` is the binding style check — if Clippy is happy, your code is in the right shape.

## Cutting a release

> Maintainer-only. Skim this if you're working on the release pipeline; skip it otherwise.

Releases are tag-driven. Pushing a `v<x.y.z>` tag triggers [`.github/workflows/release.yml`](.github/workflows/release.yml), which runs the full quality gate, builds release binaries for Linux / macOS (arm64 + x86_64) / Windows, and attaches them — plus a combined `SHA256SUMS` — to a GitHub release. The release notes come from the matching `## [<x.y.z>]` section in [CHANGELOG.md](CHANGELOG.md), so make sure the section exists before tagging.

Publishing to crates.io is **not** automated. The workflow runs `cargo publish --dry-run --locked` as a gating step so packaging breakage surfaces during the release run, but the actual push is a deliberate, manual step from the maintainer's machine. crates.io versions are irreversible — you can yank but not delete — and that bar is too high for a workflow to clear unattended.

The procedure:

```sh
# 1. Make sure main is clean and `just check` is green.
git checkout main && git pull
just check

# 2. Bump the version in Cargo.toml. Update Cargo.lock by running any cargo command:
cargo build
# `package.version` in Cargo.toml + the `[[package]]` entry in Cargo.lock both move.

# 3. Move CHANGELOG.md's `## [Unreleased]` entries into a new dated section, e.g.
#    `## [0.2.0] — 2026-06-15`. Leave a fresh empty [Unreleased] placeholder above it.

# 4. Commit + open a PR ("release: v0.2.0"). Merge after CI is green.

# 5. From main after merge, tag and push:
git checkout main && git pull
git tag -a v0.2.0 -m "v0.2.0"
git push origin v0.2.0

# 6. Wait for the release workflow to finish (binaries appear under the GitHub release).
#    The workflow also re-runs `cargo publish --dry-run --locked`; if that step fails,
#    fix forward (don't push to crates.io until the workflow is green).

# 7. From your local clone, publish to crates.io:
cargo publish --locked
```

The tag-matching check inside the workflow refuses to release if the tag doesn't match `Cargo.toml`'s `version`, so a stale-tag mistake fails fast before binaries are built.

## License

By contributing, you agree that your contributions will be licensed under the same dual MIT OR Apache-2.0 terms as the rest of the project. See [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).

## Code of Conduct

Participation in this project is governed by our [Code of Conduct](CODE_OF_CONDUCT.md). Please read it.

## Security

Found a security issue? Please don't file a public issue — see [SECURITY.md](SECURITY.md) for how to report it privately.

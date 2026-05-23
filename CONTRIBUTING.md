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

```sh
git clone https://github.com/erbiker/jtr
cd jtr
cargo build
just --list    # see available project commands
```

## The development loop

```sh
just check       # cargo fmt --check + cargo clippy + cargo test (the full quality gate)
just smoke       # end-to-end install/list/remove against the bundled sample index
just validate-index   # confirm every recipe in jtr-index/ produces valid just syntax
```

`just check` is what CI runs. If it passes locally, your PR should pass CI.

When iterating, you can run pieces individually:

```sh
cargo test                                          # unit + integration tests
cargo test --test cli install_appends               # a specific integration test
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all
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

## Code style

- **Errors:** use `anyhow::Result` with `.with_context(...)` for context. Don't introduce custom error enums unless we need programmatic discrimination.
- **Comments:** explain *why*, not *what*. If the code's intent isn't clear from naming, improve the naming first; reach for a comment only when there's a subtle invariant or workaround.
- **No premature abstractions.** Three similar lines is fine. Pull out a helper when there's a third use.

`cargo clippy -D warnings` is the binding style check — if Clippy is happy, your code is in the right shape.

## License

By contributing, you agree that your contributions will be licensed under the same dual MIT OR Apache-2.0 terms as the rest of the project. See [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).

## Code of Conduct

Participation in this project is governed by our [Code of Conduct](CODE_OF_CONDUCT.md). Please read it.

## Security

Found a security issue? Please don't file a public issue — see [SECURITY.md](SECURITY.md) for how to report it privately.

# jtr

> A package registry and CLI for sharing reusable [`just`](https://github.com/casey/just) and [`task`](https://github.com/go-task/task) recipes.

[![CI](https://github.com/erbiker/jtr/actions/workflows/ci.yml/badge.svg)](https://github.com/erbiker/jtr/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

`just` and `task` are loved tools, but every project reimplements the same recipes: dev containers, lint+format, deploys, migrations, you name it. Sharing today is copy-paste from blog posts. **`jtr` is the missing thin registry + UX layer** — vetted, versioned, and installable in one command:

```sh
jtr init                   # scaffold a justfile in a brand-new project
jtr search docker          # browse the registry
jtr install postgres-dev   # add a recipe to your justfile
jtr update postgres-dev    # pull the latest version (or `jtr update` for all)
jtr list                   # see what you've installed
jtr doctor                 # check for stale blocks, drift, missing tools
jtr remove postgres-dev    # take it back out
```

`jtr` writes its recipes into your `justfile` as a sentinel-delimited managed block — your hand-written recipes are untouched, and you can re-render or remove an installed block at any time.

> **Status: pre-alpha.** APIs and recipe formats will change. Pin your installs and expect rough edges. We're iterating in public; feedback in [Discussions](https://github.com/erbiker/jtr/discussions) is genuinely valued.

## Design philosophy

**The npm of justfiles.** `jtr` is a registry, search, and install layer — not a runtime. The transport stays whatever each tool natively supports (file imports for `just`, remote includes for `task`), so `jtr`-installed recipes are just normal entries in your project file. Uninstall `jtr` tomorrow and your recipes still work.

The registry itself is a GitHub-backed JSON catalog, mirroring the Homebrew taps model. No central servers to operate, no auth required to read, every change to the curated index goes through a PR — security review built into the workflow. The registry currently lives in `jtr-index/` inside this repo; before v1.0 we plan to split it into its own repository so recipe contributions don't require touching CLI code. See [CONTRIBUTING.md](CONTRIBUTING.md#contributing-a-recipe) for details.

## Install

### From source (today)

```sh
git clone https://github.com/erbiker/jtr
cd jtr
cargo install --path .
```

Requires Rust 1.95 or newer.

### From crates.io

```sh
cargo install jtr
```

### From GitHub Releases (prebuilt binary)

Grab a tarball/zip from the [latest release](https://github.com/erbiker/jtr/releases/latest) for your platform (Linux x86_64, macOS arm64, macOS x86_64, Windows x86_64), verify it against the included `SHA256SUMS`, extract, and drop `jtr` onto your `PATH`:

```sh
# example: macOS arm64 — adjust VERSION + TARGET for your platform
VERSION=0.1.0
TARGET=aarch64-apple-darwin
ARCHIVE=jtr-${VERSION}-${TARGET}.tar.gz
BASE=https://github.com/erbiker/jtr/releases/download/v${VERSION}

curl -fLO "${BASE}/${ARCHIVE}"
curl -fL "${BASE}/SHA256SUMS" | grep "${ARCHIVE}" | shasum -a 256 -c -
tar -xzf "${ARCHIVE}"
mv "jtr-${VERSION}-${TARGET}/jtr" ~/.local/bin/   # or anywhere on PATH
```

### From Homebrew

Not yet — deferred until v1.0. Use `cargo install jtr` or a release binary in the meantime.

## Usage

### Start a fresh project

```sh
cd ~/code/my-new-project
jtr init                  # creates ./justfile with a `default:` recipe that lists tasks
jtr init --target task    # ...or ./Taskfile.yml for the `task` runner
```

`jtr init` refuses to overwrite an existing `justfile` (or `Taskfile.yml`), so it's safe to run in a directory you're not sure about.

### Search the registry

```sh
jtr search                 # list everything
jtr search postgres        # substring match on name + description
```

### Install a recipe

```sh
cd ~/code/my-project
jtr install postgres-dev
```

This appends a block to your `justfile`:

```just
# >>> jtr:postgres-dev@0.1.0 >>>
# source: https://github.com/erbiker/jtr-index/tree/main/recipes/postgres-dev
# do not edit manually; use `jtr update postgres-dev` or `jtr remove postgres-dev`
POSTGRES_CONTAINER := "jtr-postgres-dev"
...
postgres-up:
    @docker rm -f {{POSTGRES_CONTAINER}} 2>/dev/null || true
    ...
# <<< jtr:postgres-dev <<<
```

Then run `just postgres-up` as usual.

### Update an installed recipe

```sh
jtr update postgres-dev    # pull the latest manifest, replace the block in place
jtr update                 # update every jtr-managed recipe in this project
```

`jtr update` is byte-identical no-op when the registry version matches what's installed and the managed block is unchanged. If you hand-edited the managed block, `jtr update` reverts it to canonical (`✓ refreshed postgres-dev @0.1.0`).

### Pin to a specific version

For reproducible setups across machines or CI runs, pin an install with `@<version>`:

```sh
jtr install postgres-dev@0.1.0
```

The managed block records the pin (`# pinned: 0.1.0`); `jtr update postgres-dev` then refuses to bump the block. To bump it later, either:

```sh
jtr update postgres-dev --unpin   # bump to the latest, drop the pin
jtr install postgres-dev          # bare install also overwrites the pin with latest
jtr install postgres-dev@0.2.0    # or re-pin to a different version
```

If you ask for a version the registry doesn't publish, the error lists what _is_ available. Pinning a recipe doesn't pin its transitive dependencies — those continue to resolve to whatever the registry currently publishes. (A full lockfile is deliberately out of scope until users ask for one.)

### List and remove

```sh
jtr list                              # show what's installed
jtr remove postgres-dev               # strip the managed block, collapse surrounding blanks
jtr remove postgres-dev --force       # remove even if other installed recipes depend on it
```

If another installed recipe depends on the one you're removing, `jtr remove` refuses and names the dependents so you can deal with them first. Pass `--force` to override.

### Recipe dependencies

A recipe manifest can declare other recipes it needs:

```json
{
  "name": "nextjs-deploy-vercel",
  "version": "0.1.0",
  "dependencies": ["node-lint-format", "alice/recipes/preflight"],
  "targets": { "just": { "snippet": "..." } }
}
```

Installing one recipe pulls in every transitive dependency in the right order. Bare names (e.g. `node-lint-format`) resolve to the curated index; `user/repo/recipe` names resolve to a configured tap. If a dependency references a tap that isn't configured, `jtr install` tells you which `jtr tap add` will fix it. Dependency cycles are caught at install time and reported with the offending chain.

### Add a community tap

Recipes that live outside the curated index — a private set, a team library, a hobbyist's collection — can be pulled in as a tap (modelled on Homebrew taps):

```sh
jtr tap add alice/recipes              # adds https://raw.githubusercontent.com/alice/recipes/main/index.json
jtr tap list                           # see what's configured
jtr search                             # tap recipes show up with their source label
jtr install alice/recipes/nice-thing   # install a tap recipe; the prefix routes it
jtr tap remove alice/recipes           # forget a tap; installed blocks stay in place
```

`jtr tap add` accepts a `--url` override for self-hosted indices and offline testing — e.g. `jtr tap add team/recipes --url file:///tmp/index.json`. Tap-installed recipes appear in `jtr list` / `jtr update` / `jtr doctor` under their full `user/repo/recipe` block name, so they're treated the same as curated recipes for every command.

Taps are stored in `taps.toml` under your platform's config directory (XDG on Linux, `~/Library/Application Support/dev.jtr.jtr/` on macOS). Set `JTR_CONFIG_DIR` to point at a sandbox if you want to inspect or test that location explicitly.

### Diagnose with `jtr doctor`

```sh
jtr doctor                 # check every installed recipe
```

`jtr doctor` walks every managed block in your project file and reports:

- **Orphaned blocks** — the recipe was removed from the registry; suggests `jtr remove`.
- **Version drift** — a newer version is available; suggests `jtr update`.
- **Missing tools** — anything from the recipe's `shells_out_to` that isn't on your `PATH`.

Exits non-zero if any problems are found, so it's safe to drop into CI as a gate.

## Integrity

Every recipe in the curated index ships with a SHA-256 hash of its manifest. On every install or update, `jtr` re-hashes the fetched manifest and refuses to write anything if the hash doesn't match. The hash is committed to the index as part of the recipe's PR, so any in-flight tampering — a compromised CDN edge, a man-in-the-middle proxy, an attacker with momentary write access to the index repo without matching write access to the manifest file — surfaces as a clear error rather than silently installing modified code.

If an entry in the index has no `sha256` field, `jtr` prints `warning: ... skipping integrity check` to stderr and proceeds. This is intentional during the v1 rollout so third-party indexes can be adopted before they're fully hashed; once the seed registry stabilizes we'll make checksums mandatory.

## Configuration

| Flag / env             | Default                                                                        | Meaning                                          |
| ---------------------- | ------------------------------------------------------------------------------ | ------------------------------------------------ |
| `--index <URL>` / `JTR_INDEX_URL` | `https://raw.githubusercontent.com/erbiker/jtr-index/main/index.json` | Registry index location. Accepts `http(s)://`, `file://`, or a local path. |
| `--file <PATH>`        | auto-detect `justfile` / `Taskfile.yml` in CWD                                 | Project file to read/write.                      |
| `JTR_CONFIG_DIR`       | platform config dir (`~/.config/jtr/` on Linux, `~/Library/Application Support/dev.jtr.jtr/` on macOS) | Where `taps.toml` is stored. Override for testing or sandboxing. |

## Local development against the bundled sample index

The repo ships with a sample registry under [`jtr-index/`](jtr-index/) for offline development:

```sh
cargo build
mkdir -p /tmp/jtr-demo && cd /tmp/jtr-demo
touch justfile
JTR_INDEX_URL=file://$OLDPWD/jtr-index/index.json $OLDPWD/target/debug/jtr search
JTR_INDEX_URL=file://$OLDPWD/jtr-index/index.json $OLDPWD/target/debug/jtr install postgres-dev
cat justfile
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full development workflow.

## Roadmap

The roadmap is tracked in [GitHub Issues](https://github.com/erbiker/jtr/issues) and discussed in [Discussions](https://github.com/erbiker/jtr/discussions). Near-term focus:

- **Task (YAML) write target** — `task` users get full install support, not just detection.
- **Split the registry into its own repo** — once external recipe contributions warrant it.

## Contributing

Contributions are very welcome. Two of the most useful things you can do:

1. **Contribute a recipe.** Open a [recipe proposal](https://github.com/erbiker/jtr/issues/new?template=recipe_proposal.md), then PR the manifest into `jtr-index/`.
2. **Try `jtr` on your project and tell us what hurts.** Bug reports and friction notes are gold — [open an issue](https://github.com/erbiker/jtr/issues/new/choose) or chat in [Discussions](https://github.com/erbiker/jtr/discussions).

See [CONTRIBUTING.md](CONTRIBUTING.md) for the development setup, PR workflow, and recipe authoring guide.

Participation in this project is governed by our [Code of Conduct](CODE_OF_CONDUCT.md). Security issues should be reported privately — see [SECURITY.md](SECURITY.md).

## License

Dual-licensed under [MIT](LICENSE-MIT) **or** [Apache 2.0](LICENSE-APACHE) at your option. By contributing, you agree your contributions will be licensed under the same terms.

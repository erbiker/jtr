use anyhow::{Context, Result, anyhow, bail};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Persisted on disk as `taps.toml` in the user's config dir. Holds the list of
/// community taps the curated index doesn't ship — same role as a Homebrew tap.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TapsConfig {
    #[serde(default, rename = "tap")]
    pub taps: Vec<Tap>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tap {
    pub name: String,
    pub url: String,
}

/// Directory `taps.toml` lives in. Honor `JTR_CONFIG_DIR` so tests can sandbox
/// it; otherwise defer to `directories::ProjectDirs` (XDG on Linux, Application
/// Support on macOS, Roaming on Windows).
pub fn config_dir() -> Result<PathBuf> {
    if let Some(custom) = std::env::var_os("JTR_CONFIG_DIR") {
        return Ok(PathBuf::from(custom));
    }
    let dirs = ProjectDirs::from("dev", "jtr", "jtr")
        .ok_or_else(|| anyhow!("could not determine a config directory for jtr"))?;
    Ok(dirs.config_dir().to_path_buf())
}

pub fn taps_file() -> Result<PathBuf> {
    Ok(config_dir()?.join("taps.toml"))
}

pub fn load() -> Result<TapsConfig> {
    let path = taps_file()?;
    if !path.exists() {
        return Ok(TapsConfig::default());
    }
    let raw =
        fs::read_to_string(&path).with_context(|| format!("could not read {}", path.display()))?;
    let config: TapsConfig =
        toml::from_str(&raw).with_context(|| format!("could not parse {}", path.display()))?;
    Ok(config)
}

pub fn save(config: &TapsConfig) -> Result<()> {
    let path = taps_file()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    let raw = toml::to_string_pretty(config).context("could not serialize taps.toml")?;
    fs::write(&path, raw).with_context(|| format!("could not write {}", path.display()))?;
    Ok(())
}

/// Validate that `name` is in `user/repo` form: exactly one `/`, both halves
/// non-empty and made of the `[a-z0-9_.-]` characters GitHub permits in slugs.
pub fn validate_tap_name(name: &str) -> Result<()> {
    let parts: Vec<&str> = name.split('/').collect();
    if parts.len() != 2 || parts.iter().any(|p| p.is_empty()) {
        bail!("tap name '{}' must be in `user/repo` form", name);
    }
    let ok = parts.iter().all(|seg| {
        seg.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    });
    if !ok {
        bail!(
            "tap name '{}' contains characters outside [a-z0-9_.-]",
            name
        );
    }
    Ok(())
}

/// Derive the default index URL for a `user/repo` GitHub tap. Assumes the
/// `main` branch and `index.json` at the repo root, mirroring how the curated
/// index is published.
pub fn default_url(name: &str) -> String {
    url_for_branch(name, "main")
}

/// Index URL for a `user/repo` tap on a specific branch, e.g. `release/v1` or
/// `feature-x`. Same shape as [`default_url`] with the branch substituted in.
pub fn url_for_branch(name: &str, branch: &str) -> String {
    format!("https://raw.githubusercontent.com/{name}/{branch}/index.json")
}

/// Split a `user/repo@branch` argument into the tap name and an optional branch.
/// The first `@` is the boundary: tap names can't contain `@` (see
/// [`validate_tap_name`]), so `user/repo@release/v1` parses unambiguously even
/// though the branch itself may contain `/`. A bare `user/repo` yields no branch.
/// An empty branch (`user/repo@`) is an error — almost certainly a typo.
pub fn split_branch(arg: &str) -> Result<(&str, Option<&str>)> {
    match arg.split_once('@') {
        Some((name, branch)) => {
            if branch.is_empty() {
                bail!("branch is empty after '@' in '{arg}'. Drop the '@' for the default branch.");
            }
            validate_branch(branch)?;
            Ok((name, Some(branch)))
        }
        None => Ok((arg, None)),
    }
}

/// A git branch name as we'll splice it into a raw.githubusercontent.com URL:
/// the `[a-zA-Z0-9_./-]` subset, which covers the real-world cases (`release/v1`,
/// `feature-x`, `v2.0`) without admitting characters that would change the URL's
/// meaning (`?` query, `#` fragment, spaces). Not a full git ref validator — and
/// it does permit `.`/`/`, so `..` passes — just enough to keep the URL
/// well-formed; the branch only ever shapes the user's own tap URL.
fn validate_branch(branch: &str) -> Result<()> {
    let ok = branch
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'));
    if !ok {
        bail!("branch '{branch}' contains characters outside [a-zA-Z0-9_./-]");
    }
    Ok(())
}

/// Whether a managed block's `name` was installed from the tap `tap_name`.
/// `install` prefixes tap recipes as `<tap-name>/<recipe>` (see
/// `sources::block_name_for`); the trailing `/` is the boundary so tap
/// `alice/recipes` doesn't match a block from `alice/recipes-extra`.
pub fn block_belongs_to_tap(block_name: &str, tap_name: &str) -> bool {
    block_name.starts_with(&format!("{tap_name}/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::TempDir;

    /// Serialises env-var manipulation across taps tests. `JTR_CONFIG_DIR` is
    /// process-global, so concurrent set/remove in cargo's default parallel
    /// test harness can produce a window where one test's `taps::save` runs
    /// against another test's already-dropped TempDir (writes EEXIST on the
    /// parent dir). Locking around the whole `set → run → remove` keeps the
    /// var consistent for each test's duration.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_config_dir<F: FnOnce()>(dir: &TempDir, f: F) {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { std::env::set_var("JTR_CONFIG_DIR", dir.path()) };
        f();
        unsafe { std::env::remove_var("JTR_CONFIG_DIR") };
    }

    #[test]
    fn load_returns_empty_when_no_file() {
        let dir = TempDir::new().unwrap();
        with_config_dir(&dir, || {
            let cfg = load().unwrap();
            assert!(cfg.taps.is_empty());
        });
    }

    #[test]
    fn save_then_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        with_config_dir(&dir, || {
            let cfg = TapsConfig {
                taps: vec![
                    Tap {
                        name: "alice/recipes".into(),
                        url: "https://example.com/alice/index.json".into(),
                    },
                    Tap {
                        name: "bob/extras".into(),
                        url: "https://example.com/bob/index.json".into(),
                    },
                ],
            };
            save(&cfg).unwrap();
            let loaded = load().unwrap();
            assert_eq!(loaded.taps, cfg.taps);
        });
    }

    #[test]
    fn validate_tap_name_accepts_user_repo() {
        assert!(validate_tap_name("alice/recipes").is_ok());
        assert!(validate_tap_name("a.b-c_d/repo").is_ok());
    }

    #[test]
    fn validate_tap_name_rejects_bad_shapes() {
        assert!(validate_tap_name("bare").is_err());
        assert!(validate_tap_name("too/many/slashes").is_err());
        assert!(validate_tap_name("/leading").is_err());
        assert!(validate_tap_name("trailing/").is_err());
        assert!(validate_tap_name("has spaces/repo").is_err());
    }

    #[test]
    fn default_url_targets_github_main() {
        assert_eq!(
            default_url("alice/recipes"),
            "https://raw.githubusercontent.com/alice/recipes/main/index.json"
        );
    }

    #[test]
    fn url_for_branch_substitutes_the_branch() {
        assert_eq!(
            url_for_branch("alice/recipes", "release/v1"),
            "https://raw.githubusercontent.com/alice/recipes/release/v1/index.json"
        );
    }

    #[test]
    fn split_branch_parses_name_and_optional_branch() {
        assert_eq!(
            split_branch("alice/recipes").unwrap(),
            ("alice/recipes", None)
        );
        assert_eq!(
            split_branch("alice/recipes@feature-x").unwrap(),
            ("alice/recipes", Some("feature-x"))
        );
        // A branch may itself contain `/`; only the first `@` is the boundary.
        assert_eq!(
            split_branch("alice/recipes@release/v1").unwrap(),
            ("alice/recipes", Some("release/v1"))
        );
    }

    #[test]
    fn split_branch_rejects_empty_or_malformed_branch() {
        assert!(split_branch("alice/recipes@").is_err());
        assert!(split_branch("alice/recipes@bad branch").is_err());
        assert!(split_branch("alice/recipes@a@b").is_err());
    }

    // Guards against ambiguity with `jtr install user/repo/recipe@1.0.0`: the pin
    // parser (`install::split_pin`) uses `rsplit_once('@')` on the *recipe*
    // argument and never sees the tap-add path, while `split_branch` uses
    // `split_once('@')` on the *tap* argument. Different inputs, different code
    // paths — this asserts the tap-add parse leaves the version-pin shape alone.
    #[test]
    fn split_branch_does_not_consume_a_version_pin_shape() {
        // What a user would pass to `tap add` is `user/repo[@branch]`, never a
        // recipe+pin — but if they fat-finger a pin here it stays in the branch
        // half and gets rejected by validate_branch rather than silently parsed.
        assert!(split_branch("alice/recipes@1.0.0").is_ok()); // looks like a branch; legal chars
        assert_eq!(
            split_branch("alice/recipes@1.0.0").unwrap(),
            ("alice/recipes", Some("1.0.0"))
        );
    }

    #[test]
    fn block_belongs_to_tap_respects_the_slash_boundary() {
        assert!(block_belongs_to_tap("alice/recipes/foo", "alice/recipes"));
        assert!(!block_belongs_to_tap(
            "alice/recipes-extra/foo",
            "alice/recipes"
        ));
        // Curated blocks (bare names) never belong to a tap.
        assert!(!block_belongs_to_tap("postgres-dev", "alice/recipes"));
    }
}

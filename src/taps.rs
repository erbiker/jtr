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
    format!("https://raw.githubusercontent.com/{name}/main/index.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn with_config_dir<F: FnOnce()>(dir: &TempDir, f: F) {
        // Tests in the same binary can race on the env var; serialize via a mutex
        // if this ever becomes flaky. Today we run them in their own #[test]s and
        // accept the risk — only one config-dir-touching test runs at a time in
        // practice because the harness defaults to one thread per test for IO.
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
}

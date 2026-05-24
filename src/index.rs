use anyhow::{Context, Result, anyhow, bail};
use colored::Colorize;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Duration;
use url::Url;

use crate::manifest::{IndexEntry, IndexFile, RecipeManifest};

const USER_AGENT: &str = concat!("jtr/", env!("CARGO_PKG_VERSION"));
const TIMEOUT: Duration = Duration::from_secs(15);

pub struct Registry {
    base: String,
    client: reqwest::blocking::Client,
}

impl Registry {
    pub fn new(index_url: &str) -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(TIMEOUT)
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self {
            base: index_url.to_string(),
            client,
        })
    }

    pub fn load_index(&self) -> Result<IndexFile> {
        let raw = self
            .fetch(&self.base)
            .with_context(|| format!("could not load index from {}", self.base))?;
        let index: IndexFile = serde_json::from_str(&raw)
            .with_context(|| format!("index at {} is not valid JSON", self.base))?;
        if index.version != 1 {
            bail!(
                "unsupported index version {} (this jtr supports v1)",
                index.version
            );
        }
        Ok(index)
    }

    pub fn load_manifest(&self, entry: &IndexEntry) -> Result<RecipeManifest> {
        let url = self.resolve_relative(&entry.manifest_url)?;
        let raw = self.fetch(&url).with_context(|| {
            format!("could not load manifest for '{}' from {}", entry.name, url)
        })?;
        verify_checksum(entry, raw.as_bytes(), &url)?;
        let manifest: RecipeManifest = serde_json::from_str(&raw)
            .with_context(|| format!("manifest for '{}' is not valid JSON", entry.name))?;
        if manifest.name != entry.name {
            bail!(
                "manifest name '{}' does not match index entry '{}'",
                manifest.name,
                entry.name
            );
        }
        Ok(manifest)
    }

    /// Resolve a possibly-relative manifest URL against the index URL.
    fn resolve_relative(&self, relative: &str) -> Result<String> {
        if relative.starts_with("http://")
            || relative.starts_with("https://")
            || relative.starts_with("file://")
        {
            return Ok(relative.to_string());
        }

        if let Ok(base_url) = Url::parse(&self.base) {
            let joined = base_url.join(relative).with_context(|| {
                format!("could not resolve '{}' against '{}'", relative, self.base)
            })?;
            return Ok(joined.to_string());
        }

        // Treat as a local path relative to the index file's directory.
        let base_path = PathBuf::from(&self.base);
        let parent = base_path
            .parent()
            .ok_or_else(|| anyhow!("cannot derive parent of index path '{}'", self.base))?;
        Ok(parent.join(relative).to_string_lossy().into_owned())
    }

    fn fetch(&self, location: &str) -> Result<String> {
        if let Some(path) = location.strip_prefix("file://") {
            return std::fs::read_to_string(path)
                .with_context(|| format!("failed to read {}", path));
        }

        if location.starts_with("http://") || location.starts_with("https://") {
            let resp = self
                .client
                .get(location)
                .send()
                .with_context(|| format!("HTTP request to {} failed", location))?;
            if !resp.status().is_success() {
                bail!("{} returned HTTP {}", location, resp.status());
            }
            return resp
                .text()
                .with_context(|| format!("could not read response body from {}", location));
        }

        // Fallback: treat as a local filesystem path.
        std::fs::read_to_string(location).with_context(|| format!("failed to read {}", location))
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        write!(&mut out, "{:02x}", b).expect("writing to String never fails");
    }
    out
}

fn verify_checksum(entry: &IndexEntry, raw: &[u8], source: &str) -> Result<()> {
    match entry.sha256.as_deref() {
        Some(expected) => {
            let actual = sha256_hex(raw);
            if actual != expected {
                bail!(
                    "manifest for '{}' failed checksum verification\n  expected: {}\n  actual:   {}\n  source:   {}",
                    entry.name,
                    expected,
                    actual,
                    source
                );
            }
        }
        None => {
            eprintln!(
                "{} manifest for '{}' has no sha256 in the index; skipping integrity check",
                "warning:".yellow(),
                entry.name
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    const VALID_MANIFEST: &str = r#"{"name":"foo","version":"0.1.0","description":"x","targets":{"just":{"snippet":"foo:\n    @echo hi\n"}}}"#;

    fn write_index(dir: &TempDir, sha: Option<&str>) -> String {
        let recipes = dir.path().join("recipes");
        fs::create_dir_all(&recipes).unwrap();
        fs::write(recipes.join("foo.json"), VALID_MANIFEST).unwrap();
        let sha_field = match sha {
            Some(s) => format!(",\"sha256\":\"{s}\""),
            None => String::new(),
        };
        let index = format!(
            r#"{{"version":1,"recipes":[{{"name":"foo","version":"0.1.0","description":"x","manifest_url":"recipes/foo.json","targets":["just"]{sha_field}}}]}}"#
        );
        let index_path = dir.path().join("index.json");
        fs::write(&index_path, index).unwrap();
        format!("file://{}", index_path.display())
    }

    #[test]
    fn load_manifest_passes_with_correct_checksum() {
        let dir = TempDir::new().unwrap();
        let url = write_index(&dir, Some(&sha256_hex(VALID_MANIFEST.as_bytes())));
        let registry = Registry::new(&url).unwrap();
        let index = registry.load_index().unwrap();
        let manifest = registry.load_manifest(&index.recipes[0]).unwrap();
        assert_eq!(manifest.name, "foo");
    }

    #[test]
    fn load_manifest_rejects_bad_checksum() {
        let bogus = "0".repeat(64);
        let dir = TempDir::new().unwrap();
        let url = write_index(&dir, Some(&bogus));
        let registry = Registry::new(&url).unwrap();
        let index = registry.load_index().unwrap();
        let err = registry.load_manifest(&index.recipes[0]).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("checksum"),
            "expected checksum error, got: {msg}"
        );
        assert!(
            msg.contains(&bogus),
            "error should name the expected hash, got: {msg}"
        );
        assert!(
            msg.contains(&sha256_hex(VALID_MANIFEST.as_bytes())),
            "error should name the actual hash, got: {msg}"
        );
    }

    #[test]
    fn load_manifest_succeeds_without_sha_for_backwards_compat() {
        let dir = TempDir::new().unwrap();
        let url = write_index(&dir, None);
        let registry = Registry::new(&url).unwrap();
        let index = registry.load_index().unwrap();
        let manifest = registry.load_manifest(&index.recipes[0]).unwrap();
        assert_eq!(manifest.name, "foo");
    }
}

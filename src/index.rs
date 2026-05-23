use anyhow::{Context, Result, anyhow, bail};
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

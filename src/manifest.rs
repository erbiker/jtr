use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexFile {
    pub version: u32,
    #[serde(default)]
    pub updated: Option<String>,
    pub recipes: Vec<IndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub name: String,
    pub version: String,
    pub description: String,
    /// Relative path or absolute URL to the full manifest.
    pub manifest_url: String,
    #[serde(default)]
    pub targets: Vec<String>,
    /// Lowercase hex-encoded SHA-256 of the manifest's raw bytes. Optional
    /// during the v1 rollout; once present, the fetched manifest must match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub maintainer: Option<String>,
    #[serde(default)]
    pub shells_out_to: Vec<String>,
    /// Other recipes that must be installed alongside this one. Bare names
    /// resolve to the curated index; `user/repo/recipe` names resolve to the
    /// named tap (which must be configured via `jtr tap add`).
    #[serde(default)]
    pub dependencies: Vec<String>,
    pub targets: BTreeMap<String, RecipeTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeTarget {
    /// The literal text to splice into the user's project file.
    pub snippet: String,
}

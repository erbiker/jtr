use anyhow::{Context, Result};
use colored::Colorize;

use crate::index::Registry;
use crate::manifest::{IndexEntry, IndexFile};
use crate::taps;

/// Label assigned to recipes that come from the project's curated index, as
/// opposed to a tap. Also used as the on-disk block-name prefix-discriminator
/// (curated blocks have no prefix, tap blocks have `<tap-name>/`).
pub const CURATED: &str = "curated";

/// One loaded registry, tagged with the source label that produced it.
pub struct Source {
    pub label: String,
    pub registry: Registry,
    pub index: IndexFile,
}

/// The curated registry plus every configured tap, loaded into memory.
pub struct Sources {
    pub sources: Vec<Source>,
}

impl Sources {
    /// Load curated and all configured taps. Taps that fail to load are skipped
    /// with a warning rather than failing the whole command — the curated index
    /// alone is still useful, and one broken tap shouldn't brick `jtr search`.
    pub fn load(curated_url: &str) -> Result<Self> {
        let mut sources = Vec::new();

        let curated_reg = Registry::new(curated_url)?;
        let curated_index = curated_reg
            .load_index()
            .with_context(|| format!("could not load curated index from {curated_url}"))?;
        sources.push(Source {
            label: CURATED.to_string(),
            registry: curated_reg,
            index: curated_index,
        });

        let config = taps::load()?;
        for tap in &config.taps {
            let reg = match Registry::new(&tap.url) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!(
                        "{} tap '{}' skipped: {:#}",
                        "warning:".yellow(),
                        tap.name,
                        e
                    );
                    continue;
                }
            };
            match reg.load_index() {
                Ok(index) => sources.push(Source {
                    label: tap.name.clone(),
                    registry: reg,
                    index,
                }),
                Err(e) => {
                    eprintln!(
                        "{} tap '{}' is unreachable, skipping: {:#}",
                        "warning:".yellow(),
                        tap.name,
                        e
                    );
                }
            }
        }

        Ok(Self { sources })
    }

    /// Locate a recipe by the name a user typed on the CLI, or by a managed
    /// block's `block.name`. Two forms are accepted:
    ///
    /// - `recipe-name` — curated only. Taps are ignored to keep installs explicit.
    /// - `tap-user/tap-repo/recipe-name` — only matches the named tap.
    pub fn find(&self, qualified: &str) -> Option<(&Source, &IndexEntry)> {
        let slash_count = qualified.chars().filter(|c| *c == '/').count();
        if slash_count == 2 {
            // tap-prefixed: split into "<user/repo>" and "<recipe>".
            let (tap_label, recipe_name) = qualified.rsplit_once('/')?;
            if tap_label.is_empty() || recipe_name.is_empty() {
                return None;
            }
            let source = self.sources.iter().find(|s| s.label == tap_label)?;
            let entry = source
                .index
                .recipes
                .iter()
                .find(|r| r.name == recipe_name)?;
            return Some((source, entry));
        }

        if slash_count == 0 {
            let curated = self.sources.iter().find(|s| s.label == CURATED)?;
            let entry = curated.index.recipes.iter().find(|r| r.name == qualified)?;
            return Some((curated, entry));
        }

        None
    }
}

/// The block name that `install` writes for a recipe from `source`. Curated
/// recipes use the bare name; tap recipes are prefixed with the tap's label so
/// they can't collide with curated recipes (or with other taps).
pub fn block_name_for(source_label: &str, recipe_name: &str) -> String {
    if source_label == CURATED {
        recipe_name.to_string()
    } else {
        format!("{source_label}/{recipe_name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_name_curated_is_bare() {
        assert_eq!(block_name_for(CURATED, "postgres-dev"), "postgres-dev");
    }

    #[test]
    fn block_name_tap_is_prefixed() {
        assert_eq!(
            block_name_for("alice/recipes", "extra-thing"),
            "alice/recipes/extra-thing"
        );
    }
}

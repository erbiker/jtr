use anyhow::{Context, Result, bail};
use colored::Colorize;

use crate::index::Registry;
use crate::managed;
use crate::target;

pub fn run(index_url: &str, name: Option<&str>, file_override: Option<&str>) -> Result<()> {
    let (path, target) = target::resolve(file_override)?;
    let existing = std::fs::read_to_string(&path)
        .with_context(|| format!("could not read {}", path.display()))?;

    let blocks = managed::parse_all(&existing);

    let names_to_update: Vec<String> = if let Some(n) = name {
        managed::validate_name(n)?;
        if !blocks.iter().any(|b| b.name == n) {
            bail!(
                "'{}' is not installed in {}. Use `jtr install {}` instead.",
                n,
                path.display(),
                n
            );
        }
        vec![n.to_string()]
    } else if blocks.is_empty() {
        println!(
            "{} no jtr-managed recipes installed in {}",
            "i".cyan(),
            path.display()
        );
        return Ok(());
    } else {
        blocks.iter().map(|b| b.name.clone()).collect()
    };

    let registry = Registry::new(index_url)?;
    let index = registry.load_index()?;

    let mut current_doc = existing.clone();
    let mut wrote_anything = false;

    for recipe_name in &names_to_update {
        let installed_version = blocks
            .iter()
            .find(|b| &b.name == recipe_name)
            .map(|b| b.version.clone())
            .expect("name came from blocks above");

        let Some(entry) = index.recipes.iter().find(|r| &r.name == recipe_name) else {
            println!(
                "{} {} {} no longer in registry — use `jtr remove {}` to clean up",
                "!".yellow(),
                recipe_name.bold(),
                "—".dimmed(),
                recipe_name
            );
            continue;
        };

        let manifest = registry.load_manifest(entry)?;

        let target_key = target.as_str();
        let target_recipe = manifest.targets.get(target_key).ok_or_else(|| {
            anyhow::anyhow!(
                "recipe '{}' does not support target '{}' (supports: {})",
                manifest.name,
                target_key,
                manifest
                    .targets
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;

        let rendered = managed::render(
            &manifest.name,
            &manifest.version,
            manifest.homepage.as_deref().or(Some(index_url)),
            &target_recipe.snippet,
        );

        let new_doc = managed::upsert(&current_doc, &manifest.name, &rendered)?;

        if new_doc == current_doc {
            println!(
                "{} {} {} already at version {}",
                "✓".green(),
                manifest.name.bold(),
                "—".dimmed(),
                manifest.version
            );
        } else if installed_version != manifest.version {
            println!(
                "{} updated {} {} → {}",
                "✓".green(),
                manifest.name.bold(),
                format!("@{}", installed_version).dimmed(),
                manifest.version
            );
            current_doc = new_doc;
            wrote_anything = true;
        } else {
            println!(
                "{} refreshed {} {} (reverted manual edits to managed block)",
                "✓".green(),
                manifest.name.bold(),
                format!("@{}", manifest.version).dimmed()
            );
            current_doc = new_doc;
            wrote_anything = true;
        }
    }

    if wrote_anything {
        std::fs::write(&path, &current_doc)
            .with_context(|| format!("could not write {}", path.display()))?;
    }

    Ok(())
}

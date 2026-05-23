use anyhow::{Context, Result, bail};
use colored::Colorize;

use crate::index::Registry;
use crate::managed;
use crate::target;

pub fn run(index_url: &str, name: &str, file_override: Option<&str>) -> Result<()> {
    managed::validate_name(name)?;

    let (path, target) = target::resolve(file_override)?;
    let registry = Registry::new(index_url)?;
    let index = registry.load_index()?;

    let entry = index
        .recipes
        .iter()
        .find(|r| r.name == name)
        .ok_or_else(|| {
            anyhow::anyhow!("recipe '{}' not found in registry at {}", name, index_url)
        })?;

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

    // Task target is intentionally stubbed in v1 because it requires merging into existing YAML.
    if target == target::Target::Task {
        bail!(
            "{}: writing into Taskfile.yml is not yet implemented (planned for v2). \n\
             Use --file to point at a justfile, or copy the recipe manually:\n\n{}",
            "task target not yet supported".yellow(),
            target_recipe.snippet
        );
    }

    let existing = std::fs::read_to_string(&path)
        .with_context(|| format!("could not read {}", path.display()))?;

    let already_installed = managed::parse_all(&existing)
        .iter()
        .any(|b| b.name == manifest.name);

    let rendered = managed::render(
        &manifest.name,
        &manifest.version,
        manifest.homepage.as_deref().or(Some(index_url)),
        &target_recipe.snippet,
    );

    let updated = managed::upsert(&existing, &manifest.name, &rendered)?;

    if updated == existing {
        println!(
            "{} {} {} already at version {}",
            "✓".green(),
            manifest.name.bold(),
            "—".dimmed(),
            manifest.version
        );
        return Ok(());
    }

    std::fs::write(&path, updated)
        .with_context(|| format!("could not write {}", path.display()))?;

    let action = if already_installed {
        "updated"
    } else {
        "installed"
    };

    println!(
        "{} {} {} ({}) — {}",
        "✓".green(),
        action.bold(),
        manifest.name.bold(),
        manifest.version,
        path.display()
    );
    if !manifest.shells_out_to.is_empty() {
        println!(
            "  {} {}",
            "shells out to:".dimmed(),
            manifest.shells_out_to.join(", ")
        );
    }
    Ok(())
}

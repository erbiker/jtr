use anyhow::{Context, Result, bail};
use colored::Colorize;

use crate::managed;
use crate::sources::{Sources, block_name_for};
use crate::target;

pub fn run(index_url: &str, name: &str, file_override: Option<&str>) -> Result<()> {
    managed::validate_name(name)?;

    let (path, target) = target::resolve(file_override)?;
    let sources = Sources::load(index_url)?;

    let (source, entry) = sources.find(name).ok_or_else(|| {
        anyhow::anyhow!(
            "recipe '{}' not found in the curated index or any configured tap",
            name
        )
    })?;

    let manifest = source.registry.load_manifest(entry)?;
    let block_name = block_name_for(&source.label, &manifest.name);

    let target_key = target.as_str();
    let target_recipe = manifest.targets.get(target_key).ok_or_else(|| {
        anyhow::anyhow!(
            "recipe '{}' does not support target '{}' (supports: {})",
            block_name,
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
        .any(|b| b.name == block_name);

    let source_link = manifest.homepage.clone().unwrap_or_else(|| {
        // Fall back to the source's index URL so the block records where it came
        // from — particularly useful for tap recipes, where the bare recipe name
        // doesn't tell you which tap published it.
        source.registry.base().to_string()
    });

    let rendered = managed::render(
        &block_name,
        &manifest.version,
        Some(&source_link),
        &target_recipe.snippet,
    );

    let updated = managed::upsert(&existing, &block_name, &rendered)?;

    if updated == existing {
        println!(
            "{} {} {} already at version {}",
            "✓".green(),
            block_name.bold(),
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
        block_name.bold(),
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

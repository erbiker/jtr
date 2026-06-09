use anyhow::{Context, Result};

use crate::commands::install::{describe_missing, split_pin};
use crate::managed;
use crate::sources::{Sources, block_name_for};
use crate::target;

/// Print the rendered managed block that `jtr install <name>` would write into
/// the project file. Does not modify the project file. Supports `name@version`
/// to inspect a specific published version.
///
/// Resolves through the same `Sources::find_at` path as install, so curated and
/// tap recipes work identically. Errors are mirrored from install — same
/// "tap not configured" diagnostic when a `user/repo/recipe` name references a
/// tap that hasn't been added.
pub fn run(
    index_url: &str,
    name: &str,
    file_override: Option<&str>,
    use_cache: bool,
) -> Result<()> {
    let (recipe_name, pin) = split_pin(name)?;
    managed::validate_name(&recipe_name)?;

    let (_path, target) = target::resolve(file_override)?;
    let sources = Sources::load(index_url, use_cache)?;

    let (source, entry) = sources
        .find_at(&recipe_name, pin.as_deref())?
        .ok_or_else(|| describe_missing(&sources, &recipe_name, &recipe_name))?;

    let manifest = source
        .registry
        .load_manifest(&entry)
        .with_context(|| format!("could not load manifest for '{recipe_name}'"))?;

    let target_key = target.as_str();
    let target_recipe = manifest.targets.get(target_key).ok_or_else(|| {
        anyhow::anyhow!(
            "recipe '{}' does not support target '{}' (supports: {})",
            recipe_name,
            target_key,
            manifest
                .targets
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;

    let block_name = block_name_for(&source.label, &manifest.name);
    let source_link = manifest
        .homepage
        .clone()
        .unwrap_or_else(|| source.registry.base().to_string());

    let rendered = managed::render_block(
        target,
        &block_name,
        &manifest.version,
        Some(&source_link),
        pin.as_deref(),
        &manifest.dependencies,
        &target_recipe.snippet,
    );

    print!("{rendered}");
    Ok(())
}

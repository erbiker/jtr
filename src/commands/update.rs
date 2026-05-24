use anyhow::{Context, Result, bail};
use colored::Colorize;

use crate::managed::{self, ManagedBlock};
use crate::sources::Sources;
use crate::target;

pub fn run(
    index_url: &str,
    name: Option<&str>,
    unpin: bool,
    file_override: Option<&str>,
) -> Result<()> {
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

    let sources = Sources::load(index_url)?;

    let mut current_doc = existing.clone();
    let mut wrote_anything = false;

    for block_name in &names_to_update {
        let block = blocks
            .iter()
            .find(|b| &b.name == block_name)
            .expect("name came from blocks above");

        update_one(
            &sources,
            block,
            target.as_str(),
            unpin,
            &mut current_doc,
            &mut wrote_anything,
        )?;
    }

    if wrote_anything {
        std::fs::write(&path, &current_doc)
            .with_context(|| format!("could not write {}", path.display()))?;
    }

    Ok(())
}

fn update_one(
    sources: &Sources,
    block: &ManagedBlock,
    target_key: &str,
    unpin: bool,
    current_doc: &mut String,
    wrote_anything: &mut bool,
) -> Result<()> {
    let block_name = &block.name;

    // Pinned blocks are managed by `jtr install <name>@<version>`, not `jtr update`.
    // The user has explicitly asked for a specific version, so silently refreshing
    // them would defeat the pin. `--unpin` flips them back into the normal update
    // flow and drops the pin marker.
    if let Some(pin) = &block.pinned
        && !unpin
    {
        println!(
            "{} {} {} pinned to {} — skipping (use `jtr update {} --unpin` to bump, or `jtr install {}` for latest)",
            "i".cyan(),
            block_name.bold(),
            "—".dimmed(),
            pin.bold(),
            block_name,
            block_name
        );
        return Ok(());
    }

    let Some((source, entry)) = sources.find(block_name) else {
        println!(
            "{} {} {} no longer in registry — use `jtr remove {}` to clean up",
            "!".yellow(),
            block_name.bold(),
            "—".dimmed(),
            block_name
        );
        return Ok(());
    };

    let manifest = source.registry.load_manifest(entry)?;

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

    let source_link = manifest
        .homepage
        .clone()
        .unwrap_or_else(|| source.registry.base().to_string());

    // `--unpin` drops the pin marker; otherwise an unpinned block stays unpinned.
    let rendered = managed::render(
        block_name,
        &manifest.version,
        Some(&source_link),
        None,
        &manifest.dependencies,
        &target_recipe.snippet,
    );

    let new_doc = managed::upsert(current_doc, block_name, &rendered)?;

    let was_pinned = block.pinned.is_some();
    if new_doc == *current_doc {
        println!(
            "{} {} {} already at version {}",
            "✓".green(),
            block_name.bold(),
            "—".dimmed(),
            manifest.version
        );
    } else if was_pinned {
        println!(
            "{} unpinned {} {} → {}",
            "✓".green(),
            block_name.bold(),
            format!("@{} (pinned)", block.version).dimmed(),
            manifest.version
        );
        *current_doc = new_doc;
        *wrote_anything = true;
    } else if block.version != manifest.version {
        println!(
            "{} updated {} {} → {}",
            "✓".green(),
            block_name.bold(),
            format!("@{}", block.version).dimmed(),
            manifest.version
        );
        *current_doc = new_doc;
        *wrote_anything = true;
    } else {
        println!(
            "{} refreshed {} {} (reverted manual edits to managed block)",
            "✓".green(),
            block_name.bold(),
            format!("@{}", manifest.version).dimmed()
        );
        *current_doc = new_doc;
        *wrote_anything = true;
    }

    Ok(())
}

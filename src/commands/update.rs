use anyhow::{Context, Result, anyhow, bail};
use colored::Colorize;

use crate::commands::install::{Plan, pins_from_blocks, resolve_install_order};
use crate::managed::{self, ManagedBlock};
use crate::sources::Sources;
use crate::target;

pub fn run(
    index_url: &str,
    name: Option<&str>,
    unpin: bool,
    file_override: Option<&str>,
    use_cache: bool,
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

    let sources = Sources::load(index_url, use_cache)?;

    let mut current_doc = existing.clone();
    let mut wrote_anything = false;

    for block_name in &names_to_update {
        // Re-parse each iteration so transitive deps installed by an earlier
        // update_one call are visible to later ones (deduplicated as
        // "already at version", not re-installed).
        let blocks_now = managed::parse_all(&current_doc);
        let Some(block) = blocks_now.iter().find(|b| &b.name == block_name) else {
            // Earlier iteration removed this block somehow — nothing left to update.
            continue;
        };

        update_one(
            &sources,
            block,
            &blocks_now,
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
    blocks_now: &[ManagedBlock],
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

    if sources.find(block_name).is_none() {
        println!(
            "{} {} {} no longer in registry — use `jtr remove {}` to clean up",
            "!".yellow(),
            block_name.bold(),
            "—".dimmed(),
            block_name
        );
        return Ok(());
    }

    // Build a `block_name -> pinned` map from the *current* doc, then strip the
    // entry for this block when --unpin is in play. That way the root walks at
    // latest while other blocks' pins are still honoured for transitive deps.
    let mut installed_pins = pins_from_blocks(blocks_now);
    if unpin {
        installed_pins.remove(block_name);
    }

    let order = resolve_install_order(sources, block_name, None, &installed_pins)?;

    for plan in &order {
        apply_plan(
            plan,
            blocks_now,
            block,
            target_key,
            unpin,
            current_doc,
            wrote_anything,
        )?;
    }

    Ok(())
}

fn apply_plan(
    plan: &Plan<'_>,
    blocks_now: &[ManagedBlock],
    root_block: &ManagedBlock,
    target_key: &str,
    unpin: bool,
    current_doc: &mut String,
    wrote_anything: &mut bool,
) -> Result<()> {
    let target_recipe = plan.manifest.targets.get(target_key).ok_or_else(|| {
        anyhow!(
            "recipe '{}' does not support target '{}' (supports: {})",
            plan.block_name,
            target_key,
            plan.manifest
                .targets
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;

    let source_link = plan
        .manifest
        .homepage
        .clone()
        .unwrap_or_else(|| plan.source.registry.base().to_string());

    let rendered = managed::render(
        &plan.block_name,
        &plan.manifest.version,
        Some(&source_link),
        plan.pinned.as_deref(),
        &plan.manifest.dependencies,
        &target_recipe.snippet,
    );

    let new_doc = managed::upsert(current_doc, &plan.block_name, &rendered)?;
    let pre_existing = blocks_now.iter().find(|b| b.name == plan.block_name);
    let is_root = plan.block_name == root_block.name;
    let dep_suffix = if plan.is_dependency {
        format!(" {}", "(dependency)".dimmed())
    } else {
        String::new()
    };
    // Show `(pinned)` so the user can see a transitive dep stayed pinned across an
    // update of its dependent — without it, "already at version v" reads as a
    // happens-to-be-current, not as a deliberate pin propagation.
    let pin_suffix = if plan.pinned.is_some() {
        format!(" {}", "(pinned)".dimmed())
    } else {
        String::new()
    };

    if new_doc == *current_doc {
        println!(
            "{} {} {} already at version {}{}{}",
            "✓".green(),
            plan.block_name.bold(),
            "—".dimmed(),
            plan.manifest.version,
            pin_suffix,
            dep_suffix
        );
        return Ok(());
    }

    match pre_existing {
        None => {
            println!(
                "{} installed {} ({}){}{}",
                "✓".green(),
                plan.block_name.bold(),
                plan.manifest.version,
                pin_suffix,
                dep_suffix
            );
        }
        Some(prior) => {
            if is_root && root_block.pinned.is_some() && unpin {
                println!(
                    "{} unpinned {} {} → {}",
                    "✓".green(),
                    plan.block_name.bold(),
                    format!("@{} (pinned)", root_block.version).dimmed(),
                    plan.manifest.version
                );
            } else if prior.version != plan.manifest.version {
                println!(
                    "{} updated {} {} → {}{}{}",
                    "✓".green(),
                    plan.block_name.bold(),
                    format!("@{}", prior.version).dimmed(),
                    plan.manifest.version,
                    pin_suffix,
                    dep_suffix
                );
            } else {
                println!(
                    "{} refreshed {} {} (reverted manual edits to managed block){}{}",
                    "✓".green(),
                    plan.block_name.bold(),
                    format!("@{}", plan.manifest.version).dimmed(),
                    pin_suffix,
                    dep_suffix
                );
            }
        }
    }

    *current_doc = new_doc;
    *wrote_anything = true;
    Ok(())
}

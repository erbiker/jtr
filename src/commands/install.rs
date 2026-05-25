use anyhow::{Context, Result, anyhow, bail};
use colored::Colorize;
use std::collections::{HashMap, HashSet};

use crate::managed;
use crate::manifest::RecipeManifest;
use crate::sources::{Source, Sources, block_name_for};
use crate::target;

pub fn run(
    index_url: &str,
    name: &str,
    file_override: Option<&str>,
    use_cache: bool,
) -> Result<()> {
    let (recipe_name, pin) = split_pin(name)?;
    managed::validate_name(&recipe_name)?;

    let (path, target) = target::resolve(file_override)?;
    let sources = Sources::load(index_url, use_cache)?;

    let order = resolve_install_order(&sources, &recipe_name, pin.as_deref())?;

    let target_key = target.as_str();
    for plan in &order {
        if !plan.manifest.targets.contains_key(target_key) {
            bail!(
                "recipe '{}' does not support target '{}' (supports: {})",
                plan.block_name,
                target_key,
                plan.manifest
                    .targets
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }

    // Task target is intentionally stubbed in v1 because it requires merging into existing YAML.
    // Reached only when every recipe declares a task target — otherwise the per-recipe check
    // above produces a more specific error.
    if target == target::Target::Task {
        let first = order
            .first()
            .expect("at least one recipe must be in the install order");
        let snippet = first
            .manifest
            .targets
            .get("task")
            .map(|t| t.snippet.as_str())
            .unwrap_or("");
        bail!(
            "{}: writing into Taskfile.yml is not yet implemented (planned for v2). \n\
             Use --file to point at a justfile, or copy the recipe manually:\n\n{}",
            "task target not yet supported".yellow(),
            snippet
        );
    }

    let existing = std::fs::read_to_string(&path)
        .with_context(|| format!("could not read {}", path.display()))?;
    let already = managed::parse_all(&existing);

    let mut current_doc = existing.clone();
    let mut wrote_anything = false;
    let mut summaries: Vec<(String, &Plan, bool)> = Vec::new();

    for plan in &order {
        let target_recipe = plan
            .manifest
            .targets
            .get(target_key)
            .expect("target presence checked above");

        let source_link = plan.manifest.homepage.clone().unwrap_or_else(|| {
            // Fall back to the source's index URL so the block records where it came
            // from — particularly useful for tap recipes, where the bare recipe name
            // doesn't tell you which tap published it.
            plan.source.registry.base().to_string()
        });

        let rendered = managed::render(
            &plan.block_name,
            &plan.manifest.version,
            Some(&source_link),
            plan.pinned.as_deref(),
            &plan.manifest.dependencies,
            &target_recipe.snippet,
        );

        let new_doc = managed::upsert(&current_doc, &plan.block_name, &rendered)?;
        let action = if new_doc == current_doc {
            "already-current"
        } else if already.iter().any(|b| b.name == plan.block_name) {
            "updated"
        } else {
            "installed"
        };
        if new_doc != current_doc {
            current_doc = new_doc;
            wrote_anything = true;
        }
        summaries.push((action.to_string(), plan, plan.is_dependency));
    }

    if wrote_anything {
        std::fs::write(&path, &current_doc)
            .with_context(|| format!("could not write {}", path.display()))?;
    }

    for (action, plan, is_dep) in &summaries {
        let dep_suffix = if *is_dep {
            format!(" {}", "(dependency)".dimmed())
        } else {
            String::new()
        };
        let pin_suffix = if plan.pinned.is_some() {
            format!(" {}", "(pinned)".dimmed())
        } else {
            String::new()
        };
        match action.as_str() {
            "already-current" => println!(
                "{} {} {} already at version {}{}{}",
                "✓".green(),
                plan.block_name.bold(),
                "—".dimmed(),
                plan.manifest.version,
                pin_suffix,
                dep_suffix
            ),
            other => println!(
                "{} {} {} ({}) — {}{}{}",
                "✓".green(),
                other.bold(),
                plan.block_name.bold(),
                plan.manifest.version,
                path.display(),
                pin_suffix,
                dep_suffix
            ),
        }
        if !plan.manifest.shells_out_to.is_empty() {
            println!(
                "  {} {}",
                "shells out to:".dimmed(),
                plan.manifest.shells_out_to.join(", ")
            );
        }
    }
    Ok(())
}

struct Plan<'a> {
    block_name: String,
    source: &'a Source,
    manifest: RecipeManifest,
    /// True if this plan was pulled in transitively (not directly named by the user).
    is_dependency: bool,
    /// Set when the user asked for this specific version (`foo@1.2.0`). Only the
    /// user-requested root ever carries a pin — transitive deps stay free per
    /// the v1 design (no lockfile yet).
    pinned: Option<String>,
}

/// Split `foo@1.2.0` into `("foo", Some("1.2.0"))`. Bare names return `(name, None)`.
/// Errors if the `@version` half is empty (e.g. user typed `foo@`).
pub(crate) fn split_pin(input: &str) -> Result<(String, Option<String>)> {
    match input.rsplit_once('@') {
        Some((name, version)) => {
            if name.is_empty() {
                bail!("recipe name is empty before '@version'");
            }
            if version.is_empty() {
                bail!(
                    "version is empty after '@' in '{input}'. \
                     Use `jtr install {name}` for the latest, or `jtr install {name}@<version>` to pin."
                );
            }
            Ok((name.to_string(), Some(version.to_string())))
        }
        None => Ok((input.to_string(), None)),
    }
}

/// Topologically order `root` and every transitive dependency so each item appears
/// after the things it depends on. Errors on cycles, naming both endpoints in the
/// error message. The first element of the returned `Vec` is the leaf-most dep;
/// the last is the user-requested `root`. The `root_pin` is plumbed into the root's
/// `Plan` only — deps always resolve to latest.
fn resolve_install_order<'a>(
    sources: &'a Sources,
    root: &str,
    root_pin: Option<&str>,
) -> Result<Vec<Plan<'a>>> {
    let mut visited: HashSet<String> = HashSet::new();
    let mut order: Vec<Plan<'a>> = Vec::new();
    let mut cache: HashMap<String, RecipeManifest> = HashMap::new();

    visit(
        sources,
        root,
        root,
        root_pin,
        &mut visited,
        &mut Vec::new(),
        &mut order,
        &mut cache,
        false,
    )?;
    Ok(order)
}

#[allow(clippy::too_many_arguments)]
fn visit<'a>(
    sources: &'a Sources,
    requested_root: &str,
    name: &str,
    pin: Option<&str>,
    visited: &mut HashSet<String>,
    on_stack: &mut Vec<String>,
    order: &mut Vec<Plan<'a>>,
    cache: &mut HashMap<String, RecipeManifest>,
    is_dependency: bool,
) -> Result<()> {
    if visited.contains(name) {
        return Ok(());
    }
    if let Some(pos) = on_stack.iter().position(|n| n == name) {
        // Render the cycle as A → B → ... → A so both endpoints are visible.
        let mut chain: Vec<String> = on_stack[pos..].to_vec();
        chain.push(name.to_string());
        bail!("dependency cycle detected: {}", chain.join(" → "));
    }

    let (source, entry) = sources
        .find_at(name, pin)?
        .ok_or_else(|| describe_missing(sources, requested_root, name))?;
    let manifest = source
        .registry
        .load_manifest(&entry)
        .with_context(|| format!("could not load manifest for '{name}'"))?;

    on_stack.push(name.to_string());
    for dep in manifest.dependencies.clone() {
        managed::validate_name(&dep).with_context(|| {
            format!("recipe '{name}' declares an invalid dependency name '{dep}'")
        })?;
        visit(
            sources,
            requested_root,
            &dep,
            None,
            visited,
            on_stack,
            order,
            cache,
            true,
        )?;
    }
    on_stack.pop();

    let block_name = block_name_for(&source.label, &manifest.name);
    cache.insert(name.to_string(), manifest.clone());
    order.push(Plan {
        block_name,
        source,
        manifest,
        is_dependency,
        pinned: pin.map(|s| s.to_string()),
    });
    visited.insert(name.to_string());
    Ok(())
}

pub(crate) fn describe_missing(
    sources: &Sources,
    requested_root: &str,
    missing: &str,
) -> anyhow::Error {
    let slash_count = missing.chars().filter(|c| *c == '/').count();
    if slash_count == 2
        && let Some((tap, _)) = missing.rsplit_once('/')
        && !sources.sources.iter().any(|s| s.label == tap)
    {
        if missing == requested_root {
            return anyhow!(
                "recipe '{}' not found: tap '{}' is not configured. \
                 Run `jtr tap add {}` first.",
                missing,
                tap,
                tap
            );
        }
        return anyhow!(
            "recipe '{}' depends on '{}', but tap '{}' is not configured. \
             Run `jtr tap add {}` first.",
            requested_root,
            missing,
            tap,
            tap
        );
    }
    if missing == requested_root {
        anyhow!(
            "recipe '{}' not found in the curated index or any configured tap",
            missing
        )
    } else {
        anyhow!(
            "recipe '{}' depends on '{}', which was not found in any source",
            requested_root,
            missing
        )
    }
}

use anyhow::{Context, Result, bail};
use colored::Colorize;
use similar::{ChangeTag, TextDiff};

use crate::commands::install::{describe_missing, split_pin};
use crate::managed;
use crate::sources::{Sources, block_name_for};
use crate::target;

/// Show a unified diff between the currently-installed block (if any) and the
/// block that `jtr install <name>` would write right now. Exit code 0 if
/// identical, 1 if there's a diff — drop-in for a "are my recipes current" CI
/// check.
///
/// For pinned blocks the comparison target is the *pinned* version, not latest
/// — pinning is a deliberate freeze and diff should respect it. To see drift
/// against latest, run `diff <name>@<latest>` explicitly or `jtr update --unpin`.
pub fn run(
    index_url: &str,
    name: &str,
    file_override: Option<&str>,
    use_cache: bool,
) -> Result<()> {
    let (recipe_name, explicit_pin) = split_pin(name)?;
    managed::validate_name(&recipe_name)?;

    let (path, target) = target::resolve(file_override)?;

    if target == target::Target::Task {
        bail!(
            "writing into Taskfile.yml is not yet implemented (planned for v2). \
             Use --file to point at a justfile to compare against."
        );
    }

    let existing = std::fs::read_to_string(&path)
        .with_context(|| format!("could not read {}", path.display()))?;
    let blocks = managed::parse_all(&existing);

    let sources = Sources::load(index_url, use_cache)?;

    // First locate the recipe with no pin so we can find the block by its
    // canonical block name (which depends on curated-vs-tap, not on version).
    let (source, _top_entry) = sources
        .find(&recipe_name)
        .ok_or_else(|| describe_missing(&sources, &recipe_name, &recipe_name))?;
    let block_name = block_name_for(&source.label, &recipe_name);

    let installed = blocks.iter().find(|b| b.name == block_name);

    // Resolve the version to compare against: explicit `@version` from the CLI
    // wins, otherwise honour the block's recorded pin, otherwise latest.
    let pin: Option<String> = explicit_pin
        .clone()
        .or_else(|| installed.and_then(|b| b.pinned.clone()));

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

    let source_link = manifest
        .homepage
        .clone()
        .unwrap_or_else(|| source.registry.base().to_string());

    // The block-on-disk preserves whatever pin it was installed with; the
    // newly-rendered block reflects the same pin so a clean install of a pinned
    // recipe diffs cleanly against itself.
    let rendered_pin = if explicit_pin.is_some() {
        explicit_pin.as_deref()
    } else {
        installed.and_then(|b| b.pinned.as_deref())
    };
    let rendered = managed::render(
        &block_name,
        &manifest.version,
        Some(&source_link),
        rendered_pin,
        &manifest.dependencies,
        &target_recipe.snippet,
    );

    let installed_text = installed
        .map(|_| extract_block_text(&existing, &block_name))
        .unwrap_or_default();

    if installed_text == rendered {
        return Ok(());
    }

    print_unified_diff(&block_name, &installed_text, &rendered);
    // Use process::exit so the user sees a clean diff + non-zero exit without
    // anyhow's "Error: …" prefix that `bail!` would print. Mirrors `git diff`.
    std::process::exit(1);
}

/// Pull the raw text of the named managed block out of `doc`, including the
/// open and close marker lines. Returns an empty string if the block isn't
/// found. The returned string always ends with `\n` so it diffs cleanly against
/// `managed::render`'s output (which also ends with `\n`).
fn extract_block_text(doc: &str, name: &str) -> String {
    let open_prefix = managed::open_marker(name);
    let close = managed::close_marker(name);
    let close_trimmed = close.trim();

    let lines: Vec<&str> = doc.lines().collect();
    let Some(start) = lines
        .iter()
        .position(|l| l.trim_start().starts_with(&open_prefix))
    else {
        return String::new();
    };
    let Some(rel_end) = lines[start..]
        .iter()
        .position(|l| l.trim() == close_trimmed)
    else {
        return String::new();
    };
    let end = start + rel_end;
    let mut out = lines[start..=end].join("\n");
    out.push('\n');
    out
}

fn print_unified_diff(block_name: &str, before: &str, after: &str) {
    let diff = TextDiff::from_lines(before, after);
    let before_label = if before.is_empty() {
        format!("a/{block_name} (not installed)")
    } else {
        format!("a/{block_name}")
    };
    let after_label = format!("b/{block_name}");

    println!("{}", format!("--- {before_label}").bold());
    println!("{}", format!("+++ {after_label}").bold());

    for hunk in diff.unified_diff().context_radius(3).iter_hunks() {
        println!("{}", hunk.header().to_string().cyan());
        for change in hunk.iter_changes() {
            let line = change.to_string_lossy();
            let line = line.trim_end_matches('\n');
            match change.tag() {
                ChangeTag::Delete => println!("{}", format!("-{line}").red()),
                ChangeTag::Insert => println!("{}", format!("+{line}").green()),
                ChangeTag::Equal => println!(" {line}"),
            }
        }
    }
}

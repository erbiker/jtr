use anyhow::{Context, Result};
use colored::Colorize;
use serde::Serialize;

use crate::commands::install::{describe_missing, split_pin};
use crate::managed;
use crate::sources::{Sources, available_versions};

/// Print read-only metadata about a registry recipe: description, version
/// history, source, the binaries it shells out to (its privilege surface), its
/// declared dependencies, and the manifest checksum. Unlike `show`/`diff`, this
/// never resolves a project file — `info` works in any directory because it
/// describes the recipe, not how it would land in your justfile.
///
/// Supports `name@version` to inspect a specific published version and
/// `user/repo/recipe` for tap recipes. Errors mirror install/show — same
/// "tap not configured" and "not found" diagnostics.
pub fn run(index_url: &str, name: &str, json: bool, use_cache: bool) -> Result<()> {
    let (recipe_name, pin) = split_pin(name)?;
    managed::validate_name(&recipe_name)?;

    let sources = Sources::load(index_url, use_cache)?;

    let (source, entry) = sources
        .find_at(&recipe_name, pin.as_deref())?
        .ok_or_else(|| describe_missing(&sources, &recipe_name, &recipe_name))?;

    let manifest = source
        .registry
        .load_manifest(&entry)
        .with_context(|| format!("could not load manifest for '{recipe_name}'"))?;

    let versions = available_versions(&entry);
    let targets: Vec<&str> = manifest.targets.keys().map(String::as_str).collect();

    if json {
        let payload = InfoJson {
            name: &manifest.name,
            description: &manifest.description,
            version: &manifest.version,
            versions: &versions,
            source: &source.label,
            targets: &targets,
            dependencies: &manifest.dependencies,
            shells_out_to: &manifest.shells_out_to,
            checksum: entry.sha256.as_deref(),
            homepage: manifest.homepage.as_deref(),
            maintainer: manifest.maintainer.as_deref(),
        };
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    println!(
        "{} {}",
        manifest.name.bold(),
        format!("@{}", manifest.version).dimmed()
    );
    println!("{}", manifest.description);
    println!();

    print_row("source", &source.label);
    print_row("versions", &versions.join(", "));
    print_row("targets", &targets.join(", "));
    print_list("depends on", &manifest.dependencies);
    print_list("shells out to", &manifest.shells_out_to);
    match entry.sha256.as_deref() {
        Some(sha) => print_row("checksum", sha),
        None => print_none("checksum"),
    }
    if let Some(homepage) = manifest.homepage.as_deref() {
        print_row("homepage", homepage);
    }
    if let Some(maintainer) = manifest.maintainer.as_deref() {
        print_row("maintainer", maintainer);
    }

    Ok(())
}

/// Widest label printed below, so values line up in a column.
const LABEL_WIDTH: usize = "shells out to".len();

fn print_row(label: &str, value: &str) {
    println!(
        "  {:<width$}  {}",
        label.dimmed(),
        value.cyan(),
        width = LABEL_WIDTH
    );
}

/// Render a list value, falling back to a dim em-dash when empty so the absence
/// of dependencies / shelled-out binaries reads as "none declared", not missing.
fn print_list(label: &str, items: &[String]) {
    if items.is_empty() {
        print_none(label);
    } else {
        print_row(label, &items.join(", "));
    }
}

fn print_none(label: &str) {
    println!(
        "  {:<width$}  {}",
        label.dimmed(),
        "—".dimmed(),
        width = LABEL_WIDTH
    );
}

#[derive(Serialize)]
struct InfoJson<'a> {
    name: &'a str,
    description: &'a str,
    version: &'a str,
    versions: &'a [String],
    source: &'a str,
    targets: &'a [&'a str],
    dependencies: &'a [String],
    shells_out_to: &'a [String],
    checksum: Option<&'a str>,
    homepage: Option<&'a str>,
    maintainer: Option<&'a str>,
}

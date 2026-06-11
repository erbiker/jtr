use anyhow::{Context, Result, bail};
use clap::Subcommand;
use colored::Colorize;

use crate::index::Registry;
use crate::managed;
use crate::taps::{self, Tap, TapsConfig};
use crate::target;

#[derive(Subcommand, Debug)]
pub enum TapCommand {
    /// Add a community tap. Its recipes become visible to `jtr search` and
    /// installable via `jtr install <name>/<recipe>`.
    Add {
        /// Tap name in `user/repo` form, optionally `user/repo@branch` to pull
        /// the index from a branch other than `main` (e.g. `user/repo@release/v1`).
        /// The stored tap name stays `user/repo` either way — the branch only
        /// shapes the URL. Doubles as the namespace prefix when you install one
        /// of its recipes.
        name: String,
        /// Override the index URL. Takes precedence over the `@branch` suffix and
        /// the `main`-branch default. Useful for self-hosted indices and
        /// integration tests (point at a `file://` URL).
        #[arg(long)]
        url: Option<String>,
        /// Fetch the index once before saving and report how many recipes it
        /// holds, failing the add if it's unreachable or not a valid jtr index.
        /// Off by default so you can stage taps offline.
        #[arg(long)]
        probe: bool,
    },
    /// List configured taps.
    List,
    /// Remove a configured tap by `user/repo` name. Existing managed blocks
    /// that came from this tap are left in place; removing the tap only means
    /// future `search`/`update` calls won't see it.
    Remove {
        name: String,
        /// Remove even when the project file in the current directory still has
        /// blocks installed from this tap. Those blocks become orphaned —
        /// `jtr doctor` will then flag them as "no longer in the registry".
        #[arg(long)]
        force: bool,
    },
}

pub fn run(cmd: TapCommand) -> Result<()> {
    match cmd {
        TapCommand::Add { name, url, probe } => add(&name, url.as_deref(), probe),
        TapCommand::List => list(),
        TapCommand::Remove { name, force } => remove(&name, force),
    }
}

fn add(arg: &str, url_override: Option<&str>, probe: bool) -> Result<()> {
    let (name, branch) = taps::split_branch(arg)?;
    taps::validate_tap_name(name)?;

    let url = match (url_override, branch) {
        (Some(u), Some(_)) => {
            eprintln!(
                "{} both --url and @branch given; --url takes precedence",
                "warning:".yellow()
            );
            u.to_string()
        }
        (Some(u), None) => u.to_string(),
        (None, Some(b)) => taps::url_for_branch(name, b),
        (None, None) => taps::default_url(name),
    };

    let mut config = taps::load()?;
    if let Some(existing) = config.taps.iter().find(|t| t.name == name) {
        if existing.url == url {
            println!(
                "{} tap {} already configured ({})",
                "i".cyan(),
                name.bold(),
                existing.url.dimmed()
            );
            return Ok(());
        }
        bail!(
            "tap '{}' already configured with a different URL ({}). Remove it first with `jtr tap remove {}`.",
            name,
            existing.url,
            name
        );
    }

    if probe {
        let count = probe_index(&url)?;
        let noun = if count == 1 { "recipe" } else { "recipes" };
        println!(
            "{} reachable, {} {}",
            "✓".green(),
            count.to_string().bold(),
            noun
        );
    }

    config.taps.push(Tap {
        name: name.to_string(),
        url: url.clone(),
    });
    taps::save(&config)?;
    println!(
        "{} added tap {} {}",
        "✓".green(),
        name.bold(),
        format!("({url})").dimmed()
    );
    Ok(())
}

/// Fetch the index at `url` once (bypassing the disk cache — a probe wants a live
/// answer) and return its recipe count. Errors if unreachable or not a valid v1
/// index, so the caller can refuse to persist a broken tap.
fn probe_index(url: &str) -> Result<usize> {
    let registry = Registry::new(url, None)?;
    let index = registry
        .load_index()
        .with_context(|| format!("could not probe tap index at {url}"))?;
    Ok(index.recipes.len())
}

fn list() -> Result<()> {
    let config: TapsConfig = taps::load()?;
    if config.taps.is_empty() {
        println!(
            "{} no taps configured. Add one with `jtr tap add <user/repo>`.",
            "i".cyan()
        );
        return Ok(());
    }
    let widest = config.taps.iter().map(|t| t.name.len()).max().unwrap_or(0);
    for tap in &config.taps {
        println!(
            "  {:<width$}  {}",
            tap.name.bold(),
            tap.url.dimmed(),
            width = widest
        );
    }
    Ok(())
}

fn remove(name: &str, force: bool) -> Result<()> {
    taps::validate_tap_name(name)?;
    let mut config = taps::load()?;
    if !config.taps.iter().any(|t| t.name == name) {
        bail!(
            "tap '{}' is not configured (run `jtr tap list` to see what is)",
            name
        );
    }

    if !force {
        let dependents = installed_blocks_from_tap(name)?;
        if !dependents.is_empty() {
            bail!(
                "refusing to remove tap '{name}': the project file in this directory \
                 still has {} installed block(s) from it: {}.\n\
                 Remove those blocks first (`jtr remove <name>`), or re-run with \
                 `jtr tap remove {name} --force` to drop the tap and orphan them.",
                dependents.len(),
                dependents.join(", "),
            );
        }
    }

    config.taps.retain(|t| t.name != name);
    taps::save(&config)?;
    println!("{} removed tap {}", "✓".green(), name.bold());
    Ok(())
}

/// Names of managed blocks in the current directory's project file that were
/// installed from tap `name`. Best-effort and cwd-scoped: taps are global config
/// but a tap can have installed blocks in any number of projects — we only see
/// the one we're standing in. Returns an empty list when no project file exists.
fn installed_blocks_from_tap(name: &str) -> Result<Vec<String>> {
    let Some((path, _target)) = target::find_project_file()? else {
        return Ok(Vec::new());
    };
    let existing = std::fs::read_to_string(&path)
        .with_context(|| format!("could not read {}", path.display()))?;
    Ok(managed::parse_all(&existing)
        .into_iter()
        .filter(|b| taps::block_belongs_to_tap(&b.name, name))
        .map(|b| b.name)
        .collect())
}

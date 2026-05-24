use anyhow::{Result, bail};
use clap::Subcommand;
use colored::Colorize;

use crate::taps::{self, Tap, TapsConfig};

#[derive(Subcommand, Debug)]
pub enum TapCommand {
    /// Add a community tap. Its recipes become visible to `jtr search` and
    /// installable via `jtr install <name>/<recipe>`.
    Add {
        /// Tap name in `user/repo` form. Doubles as the namespace prefix when
        /// you install one of its recipes.
        name: String,
        /// Override the index URL. Defaults to
        /// `https://raw.githubusercontent.com/<name>/main/index.json`. Useful
        /// for self-hosted indices and integration tests (point at a `file://`
        /// URL).
        #[arg(long)]
        url: Option<String>,
    },
    /// List configured taps.
    List,
    /// Remove a configured tap by `user/repo` name. Existing managed blocks
    /// that came from this tap are left in place; removing the tap only means
    /// future `search`/`update` calls won't see it.
    Remove { name: String },
}

pub fn run(cmd: TapCommand) -> Result<()> {
    match cmd {
        TapCommand::Add { name, url } => add(&name, url.as_deref()),
        TapCommand::List => list(),
        TapCommand::Remove { name } => remove(&name),
    }
}

fn add(name: &str, url_override: Option<&str>) -> Result<()> {
    taps::validate_tap_name(name)?;
    let url = url_override
        .map(|u| u.to_string())
        .unwrap_or_else(|| taps::default_url(name));

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

fn remove(name: &str) -> Result<()> {
    taps::validate_tap_name(name)?;
    let mut config = taps::load()?;
    let before = config.taps.len();
    config.taps.retain(|t| t.name != name);
    if config.taps.len() == before {
        bail!(
            "tap '{}' is not configured (run `jtr tap list` to see what is)",
            name
        );
    }
    taps::save(&config)?;
    println!("{} removed tap {}", "✓".green(), name.bold());
    Ok(())
}

use anyhow::{Context, Result};
use colored::Colorize;

use crate::managed;
use crate::target;

pub fn run(name: &str, file_override: Option<&str>) -> Result<()> {
    managed::validate_name(name)?;

    let (path, _target) = target::resolve(file_override)?;
    let existing = std::fs::read_to_string(&path)
        .with_context(|| format!("could not read {}", path.display()))?;

    let (updated, removed) = managed::remove(&existing, name);
    if !removed {
        println!(
            "{} no managed block named '{}' in {}",
            "i".cyan(),
            name,
            path.display()
        );
        return Ok(());
    }

    std::fs::write(&path, updated)
        .with_context(|| format!("could not write {}", path.display()))?;

    println!(
        "{} removed {} from {}",
        "✓".green(),
        name.bold(),
        path.display()
    );
    Ok(())
}

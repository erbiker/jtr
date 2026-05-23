use anyhow::{Context, Result};
use colored::Colorize;

use crate::managed;
use crate::target;

pub fn run(file_override: Option<&str>) -> Result<()> {
    let (path, target) = target::resolve(file_override)?;
    let existing = std::fs::read_to_string(&path)
        .with_context(|| format!("could not read {}", path.display()))?;

    let blocks = managed::parse_all(&existing);
    if blocks.is_empty() {
        println!(
            "{} no jtr-managed recipes installed in {} ({})",
            "i".cyan(),
            path.display(),
            target.as_str()
        );
        return Ok(());
    }

    println!(
        "{} {} ({})",
        "installed:".dimmed(),
        path.display(),
        target.as_str()
    );
    for block in &blocks {
        println!(
            "  {} {}",
            block.name.bold(),
            format!("@{}", block.version).dimmed()
        );
    }
    Ok(())
}

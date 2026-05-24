use anyhow::{Context, Result, bail};
use colored::Colorize;

use crate::managed;
use crate::target;

pub fn run(name: &str, force: bool, file_override: Option<&str>) -> Result<()> {
    managed::validate_name(name)?;

    let (path, _target) = target::resolve(file_override)?;
    let existing = std::fs::read_to_string(&path)
        .with_context(|| format!("could not read {}", path.display()))?;

    if !force {
        let blocks = managed::parse_all(&existing);
        let dependents: Vec<&str> = blocks
            .iter()
            .filter(|b| b.name != name && b.dependencies.iter().any(|d| d == name))
            .map(|b| b.name.as_str())
            .collect();

        if !dependents.is_empty() {
            bail!(
                "refusing to remove '{name}': {} installed recipe(s) depend on it: {}.\n\
                 Re-run with `jtr remove {name} --force` to remove anyway, \
                 or remove the dependents first.",
                dependents.len(),
                dependents.join(", "),
            );
        }
    }

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

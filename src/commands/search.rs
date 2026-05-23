use anyhow::Result;
use colored::Colorize;

use crate::index::Registry;

pub fn run(index_url: &str, query: Option<&str>) -> Result<()> {
    let registry = Registry::new(index_url)?;
    let index = registry.load_index()?;

    let needle = query.map(|q| q.to_lowercase());
    let mut matches: Vec<_> = index
        .recipes
        .iter()
        .filter(|r| match &needle {
            None => true,
            Some(q) => {
                r.name.to_lowercase().contains(q) || r.description.to_lowercase().contains(q)
            }
        })
        .collect();
    matches.sort_by(|a, b| a.name.cmp(&b.name));

    if matches.is_empty() {
        println!(
            "{} no recipes matched '{}'",
            "i".cyan(),
            needle.as_deref().unwrap_or("")
        );
        return Ok(());
    }

    let widest = matches.iter().map(|r| r.name.len()).max().unwrap_or(0);
    for entry in matches {
        let targets = if entry.targets.is_empty() {
            "".to_string()
        } else {
            format!(" [{}]", entry.targets.join(","))
        };
        println!(
            "{:<width$}  {}  {}{}",
            entry.name.bold(),
            format!("@{}", entry.version).dimmed(),
            entry.description,
            targets.dimmed(),
            width = widest
        );
    }
    Ok(())
}

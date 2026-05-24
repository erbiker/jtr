use anyhow::Result;
use colored::Colorize;

use crate::manifest::IndexEntry;
use crate::sources::{CURATED, Sources, block_name_for};

pub fn run(index_url: &str, query: Option<&str>) -> Result<()> {
    let sources = Sources::load(index_url)?;
    let needle = query.map(|q| q.to_lowercase());

    let mut matches: Vec<(&str, &IndexEntry)> = Vec::new();
    for source in &sources.sources {
        for entry in &source.index.recipes {
            let m = match &needle {
                None => true,
                Some(q) => {
                    entry.name.to_lowercase().contains(q)
                        || entry.description.to_lowercase().contains(q)
                }
            };
            if m {
                matches.push((source.label.as_str(), entry));
            }
        }
    }
    // Stable, predictable order: curated first, then taps alphabetically; within
    // each source, recipes alphabetically.
    matches.sort_by(|a, b| {
        let key_a = (a.0 != CURATED, a.0, a.1.name.as_str());
        let key_b = (b.0 != CURATED, b.0, b.1.name.as_str());
        key_a.cmp(&key_b)
    });

    if matches.is_empty() {
        println!(
            "{} no recipes matched '{}'",
            "i".cyan(),
            needle.as_deref().unwrap_or("")
        );
        return Ok(());
    }

    // Width budget the display names. Tap recipes carry their source prefix so the
    // string the user sees is the same one they can paste into `jtr install`.
    let display_names: Vec<String> = matches
        .iter()
        .map(|(label, entry)| block_name_for(label, &entry.name))
        .collect();
    let widest_name = display_names.iter().map(|n| n.len()).max().unwrap_or(0);
    let widest_label = matches
        .iter()
        .map(|(label, _)| label.len())
        .max()
        .unwrap_or(0);

    for ((label, entry), display) in matches.iter().zip(display_names.iter()) {
        let targets = if entry.targets.is_empty() {
            String::new()
        } else {
            format!(" [{}]", entry.targets.join(","))
        };
        println!(
            "{:<width$}  {}  {}{}  {}",
            display.bold(),
            format!("@{}", entry.version).dimmed(),
            entry.description,
            targets.dimmed(),
            format!("{:<lw$}", label, lw = widest_label).dimmed(),
            width = widest_name
        );
    }
    Ok(())
}

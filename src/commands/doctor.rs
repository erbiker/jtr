use anyhow::{Context, Result, bail};
use colored::Colorize;
use std::path::{Path, PathBuf};

use crate::index::Registry;
use crate::managed::{self, ManagedBlock};
use crate::manifest::IndexFile;
use crate::target;

pub fn run(index_url: &str, file_override: Option<&str>) -> Result<()> {
    let (path, target) = target::resolve(file_override)?;
    let existing = std::fs::read_to_string(&path)
        .with_context(|| format!("could not read {}", path.display()))?;
    let blocks = managed::parse_all(&existing);

    println!(
        "{} {} ({})",
        "checking".dimmed(),
        path.display(),
        target.as_str()
    );

    if blocks.is_empty() {
        println!("{} no jtr-managed recipes to check", "i".cyan());
        return Ok(());
    }

    let registry = Registry::new(index_url)?;
    let index = registry
        .load_index()
        .with_context(|| format!("could not load registry index from {}", index_url))?;

    let mut problem_count = 0usize;
    for block in &blocks {
        problem_count += check_one(&registry, &index, block);
    }

    println!();
    if problem_count == 0 {
        println!("{} all checks passed", "✓".green());
        return Ok(());
    }

    let noun = if problem_count == 1 {
        "problem"
    } else {
        "problems"
    };
    println!(
        "{} {} {} found",
        "✗".red(),
        problem_count.to_string().bold(),
        noun
    );
    bail!("{} {} found", problem_count, noun);
}

fn check_one(registry: &Registry, index: &IndexFile, block: &ManagedBlock) -> usize {
    let Some(entry) = index.recipes.iter().find(|r| r.name == block.name) else {
        println!(
            "{} {} {} no longer in the registry — run `jtr remove {}` to clean up",
            "✗".red(),
            block.name.bold(),
            format!("@{}", block.version).dimmed(),
            block.name
        );
        return 1;
    };

    let mut problems = 0usize;
    if entry.version != block.version {
        problems += 1;
        println!(
            "{} {} {} newer version available: {}",
            "!".yellow(),
            block.name.bold(),
            format!("@{}", block.version).dimmed(),
            entry.version.bold()
        );
    } else {
        println!(
            "{} {} {} up to date",
            "✓".green(),
            block.name.bold(),
            format!("@{}", block.version).dimmed()
        );
    }

    match registry.load_manifest(entry) {
        Ok(manifest) => {
            for tool in &manifest.shells_out_to {
                match find_in_path(tool) {
                    Some(p) => println!(
                        "    {} {} found at {}",
                        "✓".green(),
                        tool.bold(),
                        p.display().to_string().dimmed()
                    ),
                    None => {
                        println!("    {} {} not found in PATH", "✗".red(), tool.bold());
                        problems += 1;
                    }
                }
            }
        }
        Err(e) => {
            println!(
                "    {} could not fetch manifest to check tools: {:#}",
                "!".yellow(),
                e
            );
            problems += 1;
        }
    }

    problems
}

/// Look up `tool` in the directories on `PATH`. On Unix, requires an executable bit.
/// Mirrors the minimal subset of `which` that doctor needs; intentionally not a
/// general-purpose helper.
fn find_in_path(tool: &str) -> Option<PathBuf> {
    if tool.is_empty() {
        return None;
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(tool);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(p) {
        Ok(meta) => meta.is_file() && meta.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_in_path_locates_a_real_tool() {
        // `sh` is part of POSIX and ships on every Unix CI runner we target.
        #[cfg(unix)]
        {
            let found = find_in_path("sh");
            assert!(found.is_some(), "expected to find `sh` on PATH");
        }
    }

    #[test]
    fn find_in_path_returns_none_for_missing_tool() {
        assert!(find_in_path("definitely-not-a-real-tool-xyz123").is_none());
    }

    #[test]
    fn find_in_path_returns_none_for_empty_input() {
        assert!(find_in_path("").is_none());
    }
}

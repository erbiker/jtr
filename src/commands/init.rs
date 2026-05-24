use anyhow::{Context, Result, bail};
use colored::Colorize;

use crate::target::Target;

const JUSTFILE_TEMPLATE: &str = "# Run `just` with no args to list every recipe.

default:
    @just --list
";

const TASKFILE_TEMPLATE: &str = "version: '3'

tasks:
  default:
    desc: List available tasks
    cmds:
      - task --list
";

const JUST_CANDIDATES: &[&str] = &["justfile", "Justfile", ".justfile"];
const TASK_CANDIDATES: &[&str] = &[
    "Taskfile.yml",
    "Taskfile.yaml",
    "taskfile.yml",
    "taskfile.yaml",
];

pub fn run(target: Target) -> Result<()> {
    let cwd = std::env::current_dir().context("could not read current directory")?;

    let (candidates, write_name, body) = match target {
        Target::Just => (JUST_CANDIDATES, "justfile", JUSTFILE_TEMPLATE),
        Target::Task => (TASK_CANDIDATES, "Taskfile.yml", TASKFILE_TEMPLATE),
    };

    for name in candidates {
        let p = cwd.join(name);
        if p.exists() {
            bail!(
                "{} already exists in {}; refusing to overwrite.",
                name,
                cwd.display()
            );
        }
    }

    let path = cwd.join(write_name);
    std::fs::write(&path, body).with_context(|| format!("could not write {}", path.display()))?;

    println!(
        "{} {} {} ({}) — {}",
        "✓".green(),
        "created".bold(),
        write_name.bold(),
        target.as_str(),
        path.display()
    );
    println!("  {} jtr install <recipe>", "next:".dimmed(),);

    Ok(())
}

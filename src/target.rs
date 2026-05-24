use anyhow::{Context, Result, anyhow};
use clap::ValueEnum;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Target {
    Just,
    Task,
}

impl Target {
    pub fn as_str(self) -> &'static str {
        match self {
            Target::Just => "just",
            Target::Task => "task",
        }
    }
}

/// Locate the project file in the current working directory, or honor an explicit override.
/// just: `justfile`, `Justfile`, `.justfile`
/// task: `Taskfile.yml`, `Taskfile.yaml`, `taskfile.yml`, `taskfile.yaml`
pub fn resolve(file_override: Option<&str>) -> Result<(PathBuf, Target)> {
    if let Some(path) = file_override {
        let p = PathBuf::from(path);
        let target = classify(&p)
            .ok_or_else(|| anyhow!("could not infer target (just/task) from filename: {}", path))?;
        return Ok((p, target));
    }

    let candidates: &[(&str, Target)] = &[
        ("justfile", Target::Just),
        ("Justfile", Target::Just),
        (".justfile", Target::Just),
        ("Taskfile.yml", Target::Task),
        ("Taskfile.yaml", Target::Task),
        ("taskfile.yml", Target::Task),
        ("taskfile.yaml", Target::Task),
    ];

    let cwd = std::env::current_dir().context("could not read current directory")?;
    for (name, target) in candidates {
        let path = cwd.join(name);
        if path.is_file() {
            return Ok((path, *target));
        }
    }

    Err(anyhow!(
        "no justfile or Taskfile.yml found in {}. Pass --file to specify one.",
        cwd.display()
    ))
}

fn classify(path: &Path) -> Option<Target> {
    let name = path.file_name()?.to_str()?.to_lowercase();
    if name.contains("justfile") {
        Some(Target::Just)
    } else if name.starts_with("taskfile") {
        Some(Target::Task)
    } else {
        None
    }
}

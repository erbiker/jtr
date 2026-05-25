use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::commands;
use crate::commands::tap::TapCommand;
use crate::target::Target;

const DEFAULT_INDEX_URL: &str =
    "https://raw.githubusercontent.com/erbiker/jtr-index/main/index.json";

#[derive(Parser, Debug)]
#[command(
    name = "jtr",
    version,
    about = "Install, update, and share reusable just and task recipes",
    long_about = None
)]
pub struct Cli {
    /// Override the registry index URL. Accepts http(s)://, file://, or a local path.
    #[arg(long, global = true, env = "JTR_INDEX_URL")]
    pub index: Option<String>,

    /// Project file to read/write. Defaults to auto-detecting a justfile or Taskfile in CWD.
    #[arg(long, global = true)]
    pub file: Option<String>,

    /// Bypass the local disk cache for this invocation (skip both read and write).
    /// Honored by every fetch-touching command — install, update, search, doctor.
    #[arg(long, global = true)]
    pub no_cache: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Scaffold a fresh justfile (or Taskfile.yml) in the current directory.
    Init {
        /// Which project file to create. Defaults to `just`.
        #[arg(long, value_enum, default_value_t = Target::Just)]
        target: Target,
    },
    /// Install a recipe into your project file.
    Install {
        /// Recipe name (e.g. postgres-dev).
        name: String,
    },
    /// Remove a previously installed recipe.
    Remove {
        name: String,
        /// Remove even when other installed recipes declare this one as a dependency.
        #[arg(long)]
        force: bool,
    },
    /// Re-fetch one or more installed recipes and replace their managed block
    /// if the registry has a newer version (or if the block has drifted).
    Update {
        /// Recipe to update. Omit to update every jtr-managed recipe in the project file.
        name: Option<String>,
        /// Treat pinned recipes as unpinned for this run: bump them to the latest
        /// version and drop the pin marker. Without this flag, pinned blocks are
        /// reported and skipped.
        #[arg(long)]
        unpin: bool,
    },
    /// List recipes already installed in the project file.
    #[command(alias = "ls")]
    List,
    /// Search the registry for recipes matching a query.
    Search {
        /// Substring matched against name and description (case-insensitive).
        query: Option<String>,
    },
    /// Diagnose the installed recipes: orphaned blocks, version drift, missing tools.
    ///
    /// Exits non-zero if any problems are found, so this is suitable as a CI gate.
    Doctor,
    /// Print the rendered managed block that `jtr install <name>` would write,
    /// without touching the project file. Useful for auditing a recipe before
    /// it lands — especially when pulling from a community tap.
    Show {
        /// Recipe name (e.g. postgres-dev or user/repo/recipe). Accepts an
        /// optional `@version` to inspect a specific published version.
        name: String,
    },
    /// Show a unified diff between the currently-installed block and the block
    /// `jtr install <name>` would write right now. Exits 0 if identical, 1 if
    /// there's a diff — drop-in for CI "are my recipes current" checks.
    Diff {
        /// Recipe name (e.g. postgres-dev or user/repo/recipe). Accepts an
        /// optional `@version` to compare against a specific published version.
        name: String,
    },
    /// Manage community taps — extra indices outside the curated registry.
    Tap {
        #[command(subcommand)]
        command: TapCommand,
    },
    /// Validate recipe manifests and (optionally) a whole tap. Designed as a
    /// drop-in CI step for tap maintainers; also runs against single manifests
    /// for quick local checks while authoring.
    Lint {
        /// Path to a recipe manifest JSON file, or — with --tap — to a tap
        /// repo root containing an index.json.
        path: PathBuf,
        /// Treat `path` as a tap repo root. Validates index.json, every
        /// referenced manifest, snippet syntax, checksum consistency, and
        /// cross-field agreement between manifest and index entry.
        #[arg(long)]
        tap: bool,
        /// Update sha256 checksums in index.json to match each referenced
        /// manifest. Requires --tap because checksums live in the index, not
        /// the manifest. Other findings are still reported but not fixed.
        #[arg(long)]
        fix: bool,
    },
    /// Scaffold a new recipe — writes a manifest skeleton and, when run inside
    /// a tap repo (cwd contains index.json), appends a stub entry to the index.
    Scaffold {
        #[command(subcommand)]
        command: ScaffoldCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum ScaffoldCommand {
    /// Create a recipe manifest skeleton ready for hand-editing.
    Recipe {
        /// Recipe name (lowercase letters, digits, dash, dot, underscore).
        name: String,
        /// Which project file the recipe targets. Defaults to `just`.
        #[arg(long, value_enum, default_value_t = Target::Just)]
        target: Target,
    },
}

pub fn run(cli: Cli) -> Result<()> {
    let index_url = cli.index.unwrap_or_else(|| DEFAULT_INDEX_URL.to_string());
    let file_override = cli.file;
    let use_cache = !cli.no_cache;

    match cli.command {
        Command::Init { target } => commands::init::run(target),
        Command::Install { name } => {
            commands::install::run(&index_url, &name, file_override.as_deref(), use_cache)
        }
        Command::Remove { name, force } => {
            commands::remove::run(&name, force, file_override.as_deref())
        }
        Command::Update { name, unpin } => commands::update::run(
            &index_url,
            name.as_deref(),
            unpin,
            file_override.as_deref(),
            use_cache,
        ),
        Command::List => commands::list::run(file_override.as_deref()),
        Command::Search { query } => commands::search::run(&index_url, query.as_deref(), use_cache),
        Command::Doctor => commands::doctor::run(&index_url, file_override.as_deref(), use_cache),
        Command::Show { name } => {
            commands::show::run(&index_url, &name, file_override.as_deref(), use_cache)
        }
        Command::Diff { name } => {
            commands::diff::run(&index_url, &name, file_override.as_deref(), use_cache)
        }
        Command::Tap { command } => commands::tap::run(command),
        Command::Lint { path, tap, fix } => commands::lint::run(path, tap, fix),
        Command::Scaffold { command } => match command {
            ScaffoldCommand::Recipe { name, target } => commands::scaffold::run(&name, target),
        },
    }
}

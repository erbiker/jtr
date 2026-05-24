use anyhow::Result;
use clap::{Parser, Subcommand};

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
    Remove { name: String },
    /// Re-fetch one or more installed recipes and replace their managed block
    /// if the registry has a newer version (or if the block has drifted).
    Update {
        /// Recipe to update. Omit to update every jtr-managed recipe in the project file.
        name: Option<String>,
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
    /// Manage community taps — extra indices outside the curated registry.
    Tap {
        #[command(subcommand)]
        command: TapCommand,
    },
}

pub fn run(cli: Cli) -> Result<()> {
    let index_url = cli.index.unwrap_or_else(|| DEFAULT_INDEX_URL.to_string());
    let file_override = cli.file;

    match cli.command {
        Command::Init { target } => commands::init::run(target),
        Command::Install { name } => {
            commands::install::run(&index_url, &name, file_override.as_deref())
        }
        Command::Remove { name } => commands::remove::run(&name, file_override.as_deref()),
        Command::Update { name } => {
            commands::update::run(&index_url, name.as_deref(), file_override.as_deref())
        }
        Command::List => commands::list::run(file_override.as_deref()),
        Command::Search { query } => commands::search::run(&index_url, query.as_deref()),
        Command::Doctor => commands::doctor::run(&index_url, file_override.as_deref()),
        Command::Tap { command } => commands::tap::run(command),
    }
}

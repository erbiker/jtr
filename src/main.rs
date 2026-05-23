use anyhow::Result;
use clap::Parser;

mod cli;
mod commands;
mod index;
mod managed;
mod manifest;
mod target;

fn main() -> Result<()> {
    let args = cli::Cli::parse();
    cli::run(args)
}

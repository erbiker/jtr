use anyhow::Result;
use clap::Parser;

mod cache;
mod cli;
mod commands;
mod index;
mod managed;
mod manifest;
mod sources;
mod taps;
mod target;

fn main() -> Result<()> {
    reset_sigpipe();
    let args = cli::Cli::parse();
    cli::run(args)
}

/// Restore the default SIGPIPE disposition. Rust sets SIGPIPE to SIG_IGN at
/// startup, which turns a write to a closed pipe into an EPIPE that makes
/// `println!` panic; with SIG_DFL, `jtr ... | head` terminates quietly the way
/// standard Unix tools do (#19).
#[cfg(unix)]
fn reset_sigpipe() {
    // SAFETY: called once at startup before any threads exist; resetting a
    // signal to its default disposition here cannot race.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn reset_sigpipe() {}

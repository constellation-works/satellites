//! Short summaries, checked.

use super::groups::summary;
use crate::error::{Error, Result};
use clap::Command;

/// The longest a one-line summary may be, in characters, so a grouped root
/// help keeps one line per command on an 80-column terminal.
pub const SUMMARY_LIMIT: usize = 70;

/// Check every subcommand of `cmd`, at every depth: its summary (the first
/// line of its `about`) is not empty and is at most `limit` characters.
/// Clap's own `help` subcommand is skipped.
///
/// A program's test calls it on its assembled parser:
///
/// ```
/// use clap::Command;
///
/// let cli = Command::new("notes")
///     .subcommand(Command::new("add").about("Add a note"))
///     .subcommand(Command::new("list").about("List the notes, newest first"));
/// cw_cli::help::check_summaries(&cli, cw_cli::help::SUMMARY_LIMIT)?;
/// # Ok::<(), cw_cli::Error>(())
/// ```
///
/// # Errors
///
/// [`Error::Summaries`] naming every command that breaks the rule.
pub fn check_summaries(cmd: &Command, limit: usize) -> Result<()> {
    let mut violations = Vec::new();
    walk(cmd, cmd.get_name(), limit, &mut violations);
    if violations.is_empty() {
        Ok(())
    } else {
        Err(Error::Summaries { limit, violations })
    }
}

fn walk(cmd: &Command, path: &str, limit: usize, violations: &mut Vec<String>) {
    for sub in cmd.get_subcommands() {
        let name = sub.get_name();
        let path = format!("{path} {name}");
        if name != "help" {
            let summary = summary(sub);
            let len = summary.chars().count();
            if summary.trim().is_empty() || len > limit {
                violations.push(format!("`{path}` ({len}): {summary:?}"));
            }
        }
        walk(sub, &path, limit, violations);
    }
}

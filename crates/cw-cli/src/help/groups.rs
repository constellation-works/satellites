//! The root command list under headings.

use clap::Command;
use clap::builder::StyledStr;
use std::fmt::Write as _;
use unicode_width::UnicodeWidthStr;

/// A placeholder heading, swapped for the groups once clap has rendered the
/// rest of the help around it.
const PLACEHOLDER: &str = "__CW_CLI_COMMANDS__";

/// The gap between a command's name column and its summary.
const GAP: usize = 2;

/// One heading of the root help and the commands under it, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Group<'a> {
    /// The heading, such as `Notes`; it is written followed by `:`.
    pub heading: &'a str,
    /// The names of the subcommands under it. A name the command does not
    /// have, or a hidden one, is left out.
    pub commands: &'a [&'a str],
}

/// `cmd` with its root help listing its subcommands under `groups`, each
/// with its one-line summary, instead of clap's flat `Commands:` list.
///
/// Summaries wrap to `width` (the terminal's, from
/// [`Terminal::width`](crate::Terminal::width)), under their own column;
/// with no width each stays on one line. Clap's usage, options and footer
/// are kept as clap renders them. A subcommand in no group is not listed.
#[must_use]
pub fn grouped(cmd: Command, groups: &[Group<'_>], width: Option<usize>) -> Command {
    let mut preview = cmd.clone().subcommand_help_heading(PLACEHOLDER);
    let help = preview.render_help().to_string();
    let marker = format!("\n{PLACEHOLDER}:\n");
    let Some((before, rest)) = help.split_once(&marker) else {
        return cmd;
    };
    let after = rest.split_once("\n\n").map_or("", |(_, after)| after);
    let mut rendered = String::from(before);
    rendered.push('\n');
    rendered.push_str(&render(&preview, groups, width));
    rendered.push_str(after);
    cmd.override_help(StyledStr::from(rendered))
}

/// The groups as text, each followed by a blank line.
fn render(cmd: &Command, groups: &[Group<'_>], width: Option<usize>) -> String {
    let rows: Vec<Vec<(&str, String)>> = groups
        .iter()
        .map(|group| {
            group
                .commands
                .iter()
                .filter_map(|&name| {
                    cmd.find_subcommand(name)
                        .filter(|sub| !sub.is_hide_set())
                        .map(|sub| (name, summary(sub)))
                })
                .collect()
        })
        .collect();
    let longest = rows
        .iter()
        .flatten()
        .map(|(name, _)| name.width())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for (group, rows) in groups.iter().zip(&rows) {
        if rows.is_empty() {
            continue;
        }
        let _ = writeln!(out, "{}:", group.heading);
        for (name, summary) in rows {
            write_row(&mut out, name, summary, longest, width);
        }
        out.push('\n');
    }
    out
}

/// The first line of the command's `about`, or nothing.
pub(super) fn summary(cmd: &Command) -> String {
    cmd.get_about()
        .map(|about| about.to_string())
        .and_then(|about| about.lines().next().map(str::to_owned))
        .unwrap_or_default()
}

/// One command's row: `  <name>  <summary>`, the summary wrapped at word
/// boundaries under its own column when a width is given. When the column
/// leaves no room for its longest word, the summary goes under the name.
fn write_row(out: &mut String, name: &str, summary: &str, longest: usize, width: Option<usize>) {
    let pad = " ".repeat(longest.saturating_sub(name.width()) + GAP);
    let column = 2 + longest + GAP;
    let Some(width) = width else {
        let _ = writeln!(out, "  {name}{pad}{summary}");
        return;
    };
    let longest_word = summary
        .split_whitespace()
        .map(str::width)
        .max()
        .unwrap_or(0);
    let indent = if longest_word > width.saturating_sub(column) {
        let _ = writeln!(out, "  {name}");
        out.push_str("    ");
        4
    } else {
        let _ = write!(out, "  {name}{pad}");
        column
    };
    let mut used = indent;
    for word in summary.split_whitespace() {
        if used > indent {
            if used + 1 + word.width() > width {
                let _ = write!(out, "\n{:indent$}", "");
                used = indent;
            } else {
                out.push(' ');
                used += 1;
            }
        }
        out.push_str(word);
        used += word.width();
    }
    out.push('\n');
}

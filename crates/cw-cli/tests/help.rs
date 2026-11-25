//! Grouped root help, and the summary check a program's test calls.

use clap::Command;
use cw_cli::Error;
use cw_cli::help::{Group, SUMMARY_LIMIT, check_summaries, grouped};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn cli() -> Command {
    Command::new("notes")
        .about("Keep short notes")
        .subcommand(Command::new("add").about("Add a note"))
        .subcommand(Command::new("list").about("List the notes, newest first\n\nMore detail."))
        .subcommand(Command::new("doctor").about("Check the store"))
}

const GROUPS: &[Group<'_>] = &[
    Group {
        heading: "Notes",
        commands: &["add", "list"],
    },
    Group {
        heading: "Maintenance",
        commands: &["doctor"],
    },
];

#[test]
fn the_root_help_lists_commands_under_headings() {
    let help = grouped(cli(), GROUPS, None).render_help().to_string();
    assert!(
        help.contains(
            "Notes:\n  add     Add a note\n  list    List the notes, newest first\n\n\
             Maintenance:\n  doctor  Check the store\n"
        ),
        "{help}"
    );
    assert!(!help.contains("Commands:"), "{help}");
    assert!(help.contains("Usage: notes"), "{help}");
    assert!(help.contains("Options:"), "{help}");
}

#[test]
fn the_check_fails_a_summary_over_the_limit() -> TestResult {
    check_summaries(&cli(), SUMMARY_LIMIT)?;
    let long = "x".repeat(SUMMARY_LIMIT + 1);
    let cli = cli().subcommand(Command::new("sync").subcommand(Command::new("push").about(long)));
    match check_summaries(&cli, SUMMARY_LIMIT) {
        Err(Error::Summaries { violations, .. }) => {
            // `sync` has no summary; `sync push` has one over the limit.
            assert_eq!(violations.len(), 2, "{violations:?}");
            assert!(
                violations[0].starts_with("`notes sync` (0)"),
                "{violations:?}"
            );
            assert!(
                violations[1].starts_with("`notes sync push` (71)"),
                "{violations:?}"
            );
        }
        other => panic!("expected the summaries to fail: {other:?}"),
    }
    Ok(())
}

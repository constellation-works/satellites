//! The terminal decision, from a constructed environment snapshot.

use cw_app::Env;
use cw_cli::{Format, Mode, Probe, Terminal};

fn decide(vars: &[(&str, &str)], probe: Probe, requested: Option<Format>) -> Terminal {
    Terminal::decide(
        &Env::from_vars(vars.iter().copied()),
        "notes",
        probe,
        requested,
    )
}

const TTY: Probe = Probe {
    stdout: true,
    stderr: true,
    columns: Some(80),
};

#[test]
fn a_terminal_is_coloured() {
    assert!(decide(&[("TERM", "xterm-256color")], TTY, None).color());
}

#[test]
fn colour_is_off_when_stdout_is_not_a_terminal() {
    let terminal = decide(
        &[("TERM", "xterm"), ("CLICOLOR_FORCE", "1")],
        Probe::piped(),
        None,
    );
    assert!(!terminal.color());
    assert!(!terminal.stderr_color());
}

#[test]
fn colour_is_off_for_a_dumb_terminal() {
    assert!(!decide(&[("TERM", "dumb")], TTY, None).color());
}

#[test]
fn colour_is_off_when_no_color_is_non_empty() {
    assert!(!decide(&[("NO_COLOR", "1"), ("CLICOLOR_FORCE", "1")], TTY, None).color());
    assert!(decide(&[("NO_COLOR", "")], TTY, None).color());
}

#[test]
fn a_non_empty_clicolor_force_colours_a_terminal() {
    assert!(decide(&[("TERM", "dumb"), ("CLICOLOR_FORCE", "1")], TTY, None).color());
    assert!(!decide(&[("TERM", "dumb"), ("CLICOLOR_FORCE", "")], TTY, None).color());
}

#[test]
fn the_format_comes_from_the_flag_then_the_variable() {
    let vars = [("NOTES_FORMAT", "json")];
    assert_eq!(decide(&vars, TTY, Some(Format::Table)).mode(), Mode::Table);
    assert_eq!(decide(&vars, TTY, None).mode(), Mode::Json);
    assert_eq!(
        decide(&[("NOTES_FORMAT", "TABLE")], Probe::piped(), None).mode(),
        Mode::Table
    );
}

#[test]
fn auto_is_a_table_on_a_terminal_and_plain_lines_otherwise() {
    assert_eq!(decide(&[], TTY, None).mode(), Mode::Table);
    assert_eq!(
        decide(&[], Probe::piped(), Some(Format::Auto)).mode(),
        Mode::Plain
    );
    // An unrecognised value falls back to auto rather than failing.
    assert_eq!(
        decide(&[("NOTES_FORMAT", "yaml")], Probe::piped(), None).mode(),
        Mode::Plain
    );
}

#[test]
fn the_width_is_known_only_on_a_terminal() {
    assert_eq!(decide(&[("COLUMNS", "100")], TTY, None).width(), Some(100));
    assert_eq!(decide(&[], TTY, None).width(), Some(80));
    assert_eq!(
        decide(&[("COLUMNS", "100")], Probe::piped(), None).width(),
        None
    );
}

//! The data directory as a pure function of the captured environment.

use cw_app::{Env, Error};
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn env(vars: &[(&str, &str)]) -> Env {
    Env::from_vars(vars.iter().copied())
}

#[test]
fn an_explicit_root_beats_every_variable() -> TestResult {
    let env = env(&[("NOTES_ROOT", "/from/var"), ("HOME", "/home/u")]);
    let dir = env.app("notes").data_dir(Some(Path::new("/from/flag")));
    assert_eq!(dir?, PathBuf::from("/from/flag"));
    Ok(())
}

#[test]
fn the_root_variable_beats_home() -> TestResult {
    let env = env(&[("MY_TOOL_ROOT", "/from/var"), ("HOME", "/home/u")]);
    let dir = env.app("my-tool").data_dir(None);
    assert_eq!(dir?, PathBuf::from("/from/var"));
    Ok(())
}

#[test]
fn home_names_the_default_directory() -> TestResult {
    let env = env(&[("NOTES_ROOT", ""), ("HOME", "/home/u")]);
    let dir = env.app("notes").data_dir(None);
    assert_eq!(dir?, PathBuf::from("/home/u/.notes"));
    Ok(())
}

#[test]
fn without_home_there_is_no_data_directory() -> TestResult {
    let env = env(&[("USER", "u")]);
    let Err(error) = env.app("notes").data_dir(None) else {
        return Err("no data directory was expected".into());
    };
    assert!(matches!(&error, Error::NoDataDir { root_var } if root_var == "NOTES_ROOT"));
    assert_eq!(error.kind(), cw_base::Kind::Config);
    Ok(())
}

#[test]
fn the_table_lists_the_kit_variables_then_the_programs_own() {
    use cw_app::env::{EnvVar, table};

    const STEP: EnvVar = EnvVar::app("STEP", "How much `add` adds | per call.")
        .with_default("`1`")
        .with_flag("--step");
    const PAGER: EnvVar = EnvVar::global("PAGER", "The pager for long output.");
    const PRESET: EnvVar = EnvVar::app("PRESET", "The active preset.")
        .with_default("**standard**\n\nwhen no profile is selected");

    let expected = "\
| Variable | Meaning | Default | Flag |
|---|---|---|---|
| `MY_TOOL_ROOT` | The data directory. | `$HOME/.<app>` | `--root` |
| `MY_TOOL_FORMAT` | The output format. | — | — |
| `MY_TOOL_LOG` | The log filter. | — | — |
| `NO_COLOR` | Any non-empty value turns color off. | — | — |
| `CLICOLOR_FORCE` | Any non-empty value turns color on for a terminal, even a `dumb` one; `NO_COLOR` still wins. | — | — |
| `TERM` | The terminal type, such as `dumb`. | — | — |
| `COLUMNS` | The width in columns that a table must fit, when it is a positive number. | — | — |
| `HOME` | The home directory; the default data directory is under it. | — | — |
| `USER` | The user the environment claims, recorded as the actor; a claim, not an identity. | — | — |
| `USERNAME` | The claimed user when the one above is unset, empty or not UTF-8, on every platform. | — | — |
| `MY_TOOL_STEP` | How much `add` adds \\| per call. | `1` | `--step` |
| `MY_TOOL_PRESET` | The active preset. | **standard**<br><br>when no profile is selected | — |
| `PAGER` | The pager for long output. | — | — |
";
    assert_eq!(table("my-tool", &[STEP, PRESET, PAGER]), expected);
}

#[test]
fn the_kit_declares_the_color_and_width_variables() {
    use cw_app::env::{CLICOLOR_FORCE, COLUMNS, KIT_VARS, VarName, table};

    assert!(KIT_VARS.contains(&CLICOLOR_FORCE));
    assert!(KIT_VARS.contains(&COLUMNS));
    assert_eq!(CLICOLOR_FORCE.name(), VarName::Global("CLICOLOR_FORCE"));
    assert_eq!(COLUMNS.name(), VarName::Global("COLUMNS"));

    let text = table("my-tool", &[]);
    assert!(text.contains(
        "| `CLICOLOR_FORCE` | Any non-empty value turns color on for a terminal, even a `dumb` one; `NO_COLOR` still wins. | — | — |\n"
    ));
    assert!(text.contains(
        "| `COLUMNS` | The width in columns that a table must fit, when it is a positive number. | — | — |\n"
    ));
}

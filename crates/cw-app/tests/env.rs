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

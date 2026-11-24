//! The child command built from a cleared environment.

use std::ffi::{OsStr, OsString};
use std::process::Command;

use crate::TempRoot;

/// The system's standard program directories, after the temp root's `bin/`
/// on a child's `PATH`, so a test can still run `sh`, `env` or `git`.
#[cfg(unix)]
const SYSTEM_PATH: &[&str] = &["/usr/bin", "/bin"];
#[cfg(not(unix))]
const SYSTEM_PATH: &[&str] = &[];

/// `program` with the environment cleared and only `HOME`, `PATH` and the
/// root variable set to `root`'s values.
pub(crate) fn isolated(program: &OsStr, root: &TempRoot) -> Command {
    let mut cmd = Command::new(program);
    cmd.env_clear()
        .env("HOME", root.home())
        .env("PATH", path_value(root))
        .env(root.root_var(), root.app_root());
    cmd
}

fn path_value(root: &TempRoot) -> OsString {
    let separator = if cfg!(windows) { ";" } else { ":" };
    let mut value = root.bin().into_os_string();
    for dir in SYSTEM_PATH {
        value.push(separator);
        value.push(dir);
    }
    value
}

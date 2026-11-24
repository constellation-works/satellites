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

/// `program` with the environment cleared, only `HOME`, `PATH`, the root
/// variable and `GIT_CEILING_DIRECTORIES` set to `root`'s values, and the
/// working directory at the root.
pub(crate) fn isolated(program: &OsStr, root: &TempRoot) -> Command {
    let mut cmd = Command::new(program);
    cmd.current_dir(root.path())
        .env_clear()
        .env("HOME", root.home())
        .env("PATH", path_value(root))
        .env(root.root_var(), root.app_root())
        .env("GIT_CEILING_DIRECTORIES", root.git_ceiling());
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

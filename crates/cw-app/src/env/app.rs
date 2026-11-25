//! One program's view of the environment: its own variables and its data
//! directory.

use super::Env;
use super::kit::{FORMAT, LOG, ROOT};
use super::var::{EnvVar, VarName, prefix};
use crate::error::{Error, Result};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The variables of one program, named from the program's name: for
/// `notes`, `NOTES_ROOT`, `NOTES_FORMAT` and `NOTES_LOG`, and the default
/// data directory `$HOME/.notes`.
///
/// The prefix is the name upper-cased, with every character that is not an
/// ASCII letter or digit written as `_` (`my-tool` reads `MY_TOOL_ROOT`).
#[derive(Clone, Debug)]
pub struct AppEnv<'a> {
    env: &'a Env,
    name: &'a str,
    prefix: String,
}

impl<'a> AppEnv<'a> {
    pub(super) fn new(env: &'a Env, name: &'a str) -> Self {
        let prefix = prefix(name);
        Self { env, name, prefix }
    }

    /// The program's name, as given.
    #[must_use]
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// The program's variable for `suffix`: `var_name("ROOT")` is
    /// `NOTES_ROOT` for `notes`.
    #[must_use]
    pub fn var_name(&self, suffix: &str) -> String {
        format!("{}_{suffix}", self.prefix)
    }

    /// The program's variable for `suffix`, or `None` when unset or empty.
    #[must_use]
    pub fn var(&self, suffix: &str) -> Option<&'a OsStr> {
        self.env.var(&self.var_name(suffix))
    }

    /// The value of the declared variable `var`, a program's own or the
    /// kit's, or `None` when it is unset or empty.
    #[must_use]
    pub fn get(&self, var: &EnvVar) -> Option<&'a OsStr> {
        match var.name() {
            VarName::Global(name) => self.env.var(name),
            VarName::App(suffix) => self.var(suffix),
        }
    }

    /// `<APP>_ROOT`, the data directory the environment names.
    #[must_use]
    pub fn root(&self) -> Option<&'a OsStr> {
        self.get(&ROOT)
    }

    /// `<APP>_FORMAT`, the output format the environment asks for.
    #[must_use]
    pub fn format(&self) -> Option<&'a OsStr> {
        self.get(&FORMAT)
    }

    /// `<APP>_LOG`, the log filter the environment asks for.
    #[must_use]
    pub fn log(&self) -> Option<&'a OsStr> {
        self.get(&LOG)
    }

    /// The data directory: `explicit` (a `--root` flag) when given, else
    /// `<APP>_ROOT`, else `$HOME/.<app>`.
    ///
    /// Pure: it reads only the captured value, and creates nothing.
    ///
    /// # Errors
    ///
    /// [`Error::NoDataDir`] when none of the three names one.
    pub fn data_dir(&self, explicit: Option<&Path>) -> Result<PathBuf> {
        if let Some(root) = explicit {
            return Ok(root.to_path_buf());
        }
        if let Some(root) = self.root() {
            return Ok(PathBuf::from(root));
        }
        self.env
            .home()
            .map(|home| home.join(format!(".{}", self.name)))
            .ok_or_else(|| Error::NoDataDir {
                root_var: ROOT.name_for(self.name),
            })
    }
}

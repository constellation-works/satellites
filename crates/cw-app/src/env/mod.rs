//! The process environment, read once.
//!
//! A program calls [`Env::capture`] once, in `main`, and hands the value
//! down. Everything that depends on a variable (the data directory, the
//! actor, whether to color output, the output format, the log filter) is a
//! pure function of that value, so a test builds one with [`Env::from_vars`]
//! and never touches the process environment. This folder is the only code
//! in the crate that names `std::env` (`scripts/check-dependency-direction.sh`).
//!
//! An empty value reads as unset everywhere, as the `NO_COLOR` convention
//! asks and as a shell's `VAR= cmd` intends.

mod app;

pub use app::AppEnv;

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::Path;

/// A snapshot of the process environment.
///
/// `Debug` shows variable names, never values: the environment can hold
/// credentials, and a debug print of a context must not leak them.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Env {
    vars: BTreeMap<OsString, OsString>,
}

impl Env {
    /// Read every variable of the process environment, once.
    #[must_use]
    pub fn capture() -> Self {
        Self::from_vars(std::env::vars_os())
    }

    /// An environment holding exactly `vars`: for a test, or a surface that
    /// received its client's environment some other way.
    #[must_use]
    pub fn from_vars<I, K, V>(vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<OsString>,
        V: Into<OsString>,
    {
        Self {
            vars: vars
                .into_iter()
                .map(|(name, value)| (name.into(), value.into()))
                .collect(),
        }
    }

    /// The value of `name`, or `None` when it is unset or empty.
    #[must_use]
    pub fn var(&self, name: &str) -> Option<&OsStr> {
        self.vars
            .get(OsStr::new(name))
            .map(OsString::as_os_str)
            .filter(|value| !value.is_empty())
    }

    /// `HOME`.
    #[must_use]
    pub fn home(&self) -> Option<&Path> {
        self.var("HOME").map(Path::new)
    }

    /// The operating-system user: `USER`, else `USERNAME` (Windows), when
    /// it is valid UTF-8.
    #[must_use]
    pub fn user(&self) -> Option<&str> {
        self.var("USER")
            .and_then(OsStr::to_str)
            .or_else(|| self.var("USERNAME").and_then(OsStr::to_str))
    }

    /// Whether `NO_COLOR` asks for output without color.
    #[must_use]
    pub fn no_color(&self) -> bool {
        self.var("NO_COLOR").is_some()
    }

    /// `TERM`, the terminal type, such as `dumb`.
    #[must_use]
    pub fn term(&self) -> Option<&OsStr> {
        self.var("TERM")
    }

    /// The variables of the program named `name`: its root, format and log
    /// variables, and its data directory.
    #[must_use]
    pub fn app<'a>(&'a self, name: &'a str) -> AppEnv<'a> {
        AppEnv::new(self, name)
    }
}

impl fmt::Debug for Env {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Env")
            .field("names", &self.vars.keys().collect::<Vec<_>>())
            .finish()
    }
}

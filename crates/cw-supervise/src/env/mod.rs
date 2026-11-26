//! Clear a child's environment and pass only what the caller names.
//!
//! The crate ships the mechanism and no list of names: which variables a
//! child gets is each tool's decision.
//!
//! ```
//! use cw_supervise::env::{self, Allow};
//! use std::process::Command;
//!
//! let mut git = Command::new("git");
//! env::allow(&mut git, &["PATH", "HOME", "LANG"]);
//!
//! // Pass the named variables and every `ORBIT_` one, but never one of this
//! // tool's own `SCRIBE_` overrides, even if it is named.
//! let mut callback = Command::new("orbit");
//! Allow::names(["PATH", "HOME"])
//!     .prefix("ORBIT_")
//!     .deny_prefix("SCRIBE_")
//!     .apply(&mut callback);
//! ```

#![expect(
    clippy::disallowed_methods,
    reason = "env Allow apply reads this process environment to build a child environment; a caller holding a snapshot uses apply_from"
)]

use std::ffi::{OsStr, OsString};
use std::process::Command;

/// Clear `command`'s environment, then copy each variable in `names` that is
/// set in this process. A name that is not set is skipped.
pub fn allow(command: &mut Command, names: &[&str]) {
    Allow::names(names).apply(command);
}

/// Which variables a child may have: named ones, and those with a prefix to
/// pass, never those with a prefix to deny.
///
/// A denied prefix wins over a name and over a passed prefix.
#[derive(Clone, Debug, Default)]
#[must_use]
pub struct Allow {
    names: Vec<OsString>,
    pass: Vec<OsString>,
    deny: Vec<OsString>,
}

impl Allow {
    /// Pass exactly these variables, when they are set.
    pub fn names<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        Self {
            names: names.into_iter().map(|n| n.as_ref().to_owned()).collect(),
            pass: Vec::new(),
            deny: Vec::new(),
        }
    }

    /// Also pass every variable whose name starts with `prefix`.
    pub fn prefix(mut self, prefix: impl AsRef<OsStr>) -> Self {
        self.pass.push(prefix.as_ref().to_owned());
        self
    }

    /// Never pass a variable whose name starts with `prefix`, even a named
    /// one: a tool's own override variables stay out of its children.
    pub fn deny_prefix(mut self, prefix: impl AsRef<OsStr>) -> Self {
        self.deny.push(prefix.as_ref().to_owned());
        self
    }

    /// Clear `command`'s environment and copy the allowed variables from
    /// this process's environment.
    pub fn apply(&self, command: &mut Command) {
        self.apply_from(command, std::env::vars_os());
    }

    /// Clear `command`'s environment and copy the allowed variables from
    /// `vars`, a snapshot the caller took (once, in `main`, say).
    pub fn apply_from<I>(&self, command: &mut Command, vars: I)
    where
        I: IntoIterator<Item = (OsString, OsString)>,
    {
        command.env_clear();
        for (name, value) in vars {
            if self.passes(&name) {
                command.env(name, value);
            }
        }
    }

    fn passes(&self, name: &OsStr) -> bool {
        let starts = |prefix: &OsString| {
            name.as_encoded_bytes()
                .starts_with(prefix.as_encoded_bytes())
        };
        if self.deny.iter().any(starts) {
            return false;
        }
        self.names.iter().any(|n| n == name) || self.pass.iter().any(starts)
    }
}

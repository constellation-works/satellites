//! Test isolation for command-line tools.
//!
//! A test that runs a program must not read or write the machine it runs on:
//! not the developer's home, not their configuration, not the variables their
//! shell exported. `cw-testkit` gives every test three things for that:
//!
//! - [`TempRoot`]: a private (`0700`) temporary directory, removed on drop,
//!   holding a home, a data root and a `bin/` directory for the test.
//! - [`TempRoot::command`]: a child [`Command`](std::process::Command) built
//!   from a cleared environment, run in the temp root. Only `HOME`, `PATH`,
//!   `<APP>_ROOT` and `GIT_CEILING_DIRECTORIES` are set, to the temp root's
//!   values; a test adds whatever else it names. Nothing else is inherited.
//! - [`Goldens`]: compares output with a file under the test's `goldens/`
//!   folder. [`UPDATE_VAR`] (`UPDATE_GOLDENS=1`) rewrites the files instead,
//!   and [`redact_timestamps`] replaces timestamps with a placeholder so the
//!   clock never makes a golden stale.
//!
//! The crate is a dev-dependency. It is synchronous and depends on no other
//! kit crate.
//!
//! ```
//! use cw_testkit::TempRoot;
//!
//! # fn main() -> Result<(), cw_testkit::Error> {
//! let root = TempRoot::new("MYAPP")?;
//! let mut cmd = root.command("env");
//! cmd.env("MYAPP_LOG", "debug");
//! // `cmd` sees HOME, PATH, MYAPP_ROOT, GIT_CEILING_DIRECTORIES and
//! // MYAPP_LOG, and nothing else.
//! assert_eq!(root.root_var(), "MYAPP_ROOT");
//! # Ok(())
//! # }
//! ```

mod child;
mod error;
mod golden;
mod root;

pub use error::Error;
pub use golden::{Goldens, TIMESTAMP_PLACEHOLDER, UPDATE_VAR, redact_timestamps};
pub use root::TempRoot;

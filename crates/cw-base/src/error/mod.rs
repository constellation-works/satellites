//! The one error type a program's units meet in.
//!
//! Every unit keeps its own typed error enum for its own matching and
//! translates it into [`Error`] once, in a `From` impl beside that enum. That
//! impl states the [`Kind`] and the stable code; everything above the unit
//! carries this one type, and a surface renders it.
//!
//! An [`Error`] is the unit's typed error, kept whole, with the two things a
//! caller needs without knowing that unit: how the failure is classified
//! ([`Kind`]) and its stable code. Its `Display` and its source chain are the
//! typed error's own, so a message is written once, where the failure is
//! known. A caller that does know the unit gets the typed error back with
//! [`Error::downcast_ref`].
//!
//! The code belongs to the project, not to a kit crate: a kit crate's error
//! has a [`Kind`] and no code, and the unit that wraps it names the code,
//! because the code is that project's contract with its users.
//!
//! ```
//! use cw_base::{Error, Kind};
//!
//! #[derive(Debug, thiserror::Error)]
//! enum StoreError {
//!     #[error("no note named {0}")]
//!     Missing(String),
//! }
//!
//! impl From<StoreError> for Error {
//!     fn from(error: StoreError) -> Self {
//!         match &error {
//!             StoreError::Missing(_) => Error::new(Kind::NotFound, "note_not_found", error),
//!         }
//!     }
//! }
//!
//! let error = Error::from(StoreError::Missing("draft".into()));
//! assert_eq!(error.kind(), Kind::NotFound);
//! assert_eq!(error.code(), "note_not_found");
//! assert_eq!(error.to_string(), "no note named draft");
//! assert!(matches!(error.downcast_ref::<StoreError>(), Some(StoreError::Missing(_))));
//! ```

mod kind;

pub use kind::Kind;

use std::error::Error as StdError;
use std::fmt;

/// A failure of any unit: the unit's typed error, its [`Kind`] and its
/// stable code.
///
/// Built only through [`Error::new`], from the unit's one translator.
#[derive(Debug)]
pub struct Error {
    kind: Kind,
    code: &'static str,
    cause: Box<dyn StdError + Send + Sync + 'static>,
}

impl Error {
    /// Wrap `cause`, a unit's typed error, with its classification and its
    /// stable `snake_case` code. Called from the unit's one translator, never
    /// ad hoc at a call site.
    #[must_use]
    pub fn new(
        kind: Kind,
        code: &'static str,
        cause: impl StdError + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind,
            code,
            cause: Box::new(cause),
        }
    }

    /// How the failure is classified: what a caller can do about it.
    #[must_use]
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// The stable `snake_case` code, for machine output and an audit log.
    /// Adding a code is fine; changing one is a breaking change of the
    /// project that names it.
    #[must_use]
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// The unit's typed error, when it is a `T`: for a caller that knows the
    /// unit and needs the variant, such as a surface choosing a remedy.
    #[must_use]
    pub fn downcast_ref<T: StdError + 'static>(&self) -> Option<&T> {
        self.cause.downcast_ref::<T>()
    }
}

/// The typed error's own sentence.
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.cause, f)
    }
}

/// The typed error's own source chain, so a surface prints each cause once.
impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.cause.source()
    }
}

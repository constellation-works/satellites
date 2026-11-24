//! `cw_base::Error` keeps a unit's typed error whole.

use cw_base::{Error, Kind};

#[derive(Debug, thiserror::Error)]
enum UnitError {
    #[error("record {name} is missing")]
    Missing { name: String },
    #[error("cannot read the record")]
    Read(#[source] std::io::Error),
}

impl From<UnitError> for Error {
    fn from(error: UnitError) -> Self {
        match &error {
            UnitError::Missing { .. } => Self::new(Kind::NotFound, "record_missing", error),
            UnitError::Read(_) => Self::new(Kind::Io, "record_read", error),
        }
    }
}

#[test]
fn the_typed_error_is_recovered_whole_with_its_kind_and_code() {
    let error = Error::from(UnitError::Missing {
        name: String::from("draft"),
    });
    assert_eq!(error.kind(), Kind::NotFound);
    assert_eq!(error.code(), "record_missing");
    assert_eq!(error.to_string(), "record draft is missing");
    let typed = error.downcast_ref::<UnitError>();
    assert!(
        matches!(typed, Some(UnitError::Missing { name }) if name == "draft"),
        "got {typed:?}"
    );
    assert!(error.downcast_ref::<std::io::Error>().is_none());
}

#[test]
fn the_source_chain_is_the_typed_errors_own() {
    let error = Error::from(UnitError::Read(std::io::Error::other("disk on fire")));
    let source = std::error::Error::source(&error).map(ToString::to_string);
    assert_eq!(source.as_deref(), Some("disk on fire"));
}

//! The format a command line asks for, and the mode it resolves to.

use clap::ValueEnum;
use clap::builder::PossibleValue;

/// The output format a command line or `<APP>_FORMAT` asks for.
///
/// It implements [`clap::ValueEnum`], so a program declares its flag as
/// `#[arg(long, value_enum)] format: Option<Format>`. The values carry no
/// per-value help: clap would switch every help page to its long layout. The
/// flag's own help names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    /// A table on a terminal, tab-separated lines otherwise.
    Auto,
    /// An aligned table with a header row, wherever stdout goes.
    Table,
    /// One JSON document.
    Json,
}

impl Format {
    /// The format a variable value names, ignoring case and surrounding
    /// whitespace, or `None` when it names none.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        [Self::Auto, Self::Table, Self::Json]
            .into_iter()
            .find(|format| format.name().eq_ignore_ascii_case(value))
    }

    /// The value as it is typed: `auto`, `table` or `json`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Table => "table",
            Self::Json => "json",
        }
    }
}

impl ValueEnum for Format {
    fn value_variants<'a>() -> &'a [Self] {
        &[Self::Auto, Self::Table, Self::Json]
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(PossibleValue::new(self.name()))
    }
}

/// How this invocation renders its payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    /// A borderless aligned table with one header row (STD-01 R14).
    Table,
    /// One tab-separated line per record and no header (STD-01 R9).
    Plain,
    /// One JSON document (STD-01 R7).
    Json,
}

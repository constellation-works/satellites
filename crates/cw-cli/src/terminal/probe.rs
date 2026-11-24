//! What the standard streams are attached to.

use std::io::{self, IsTerminal};

/// Which streams are terminals, and the width stdout's terminal reports.
///
/// [`Probe::detect`] asks the running process; a test builds one by hand.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Probe {
    /// Whether stdout is a terminal.
    pub stdout: bool,
    /// Whether stderr is a terminal.
    pub stderr: bool,
    /// The column count stdout's terminal reports, when it reports one.
    pub columns: Option<usize>,
}

impl Probe {
    /// Ask the running process.
    #[must_use]
    pub fn detect() -> Self {
        let stdout = io::stdout().is_terminal();
        Self {
            stdout,
            stderr: io::stderr().is_terminal(),
            columns: if stdout { terminal_columns() } else { None },
        }
    }

    /// Neither stream is a terminal and no width is known: the probe of a
    /// process whose output is piped.
    #[must_use]
    pub fn piped() -> Self {
        Self::default()
    }

    /// Both streams are terminals `columns` wide, or of unknown width.
    #[must_use]
    pub fn terminal(columns: Option<usize>) -> Self {
        Self {
            stdout: true,
            stderr: true,
            columns,
        }
    }
}

/// The terminal's own width (`TIOCGWINSZ`): most shells do not export
/// `COLUMNS`, so without this a table would rarely fit its terminal.
#[cfg(unix)]
fn terminal_columns() -> Option<usize> {
    let size = rustix::termios::tcgetwinsize(io::stdout()).ok()?;
    Some(usize::from(size.ws_col)).filter(|&columns| columns > 0)
}

/// No terminal query here: `COLUMNS` alone decides, which STD-01 R17 allows.
#[cfg(not(unix))]
fn terminal_columns() -> Option<usize> {
    None
}

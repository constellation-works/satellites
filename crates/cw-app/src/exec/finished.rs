//! A command's result, with its audit outcome still to write.

use crate::audit::{AuditWarning, Pending};

/// What [`super::exec()`] returns: the command's result and its pending
/// audit record.
///
/// The surface renders the result first ([`Finished::result`], or the
/// owned result from [`Finished::into_parts`]), then settles the record, so
/// the audit outcome follows the output and a failure to write it reaches the
/// surface as an [`AuditWarning`] value to print after the output.
#[must_use = "render the result, then settle the audit record"]
#[derive(Debug)]
pub struct Finished<O> {
    result: Result<O, cw_base::Error>,
    audit: Pending,
}

impl<O> Finished<O> {
    pub(super) fn new(result: Result<O, cw_base::Error>, audit: Pending) -> Self {
        Self { result, audit }
    }

    /// The command's result, to render.
    pub fn result(&self) -> Result<&O, &cw_base::Error> {
        self.result.as_ref()
    }

    /// Whether a started audit line was written for the command.
    #[must_use]
    pub fn is_audited(&self) -> bool {
        self.audit.is_audited()
    }

    /// How many items an itemized command admitted, zero for any other.
    #[must_use]
    pub fn admitted(&self) -> usize {
        self.audit.admitted()
    }

    /// Write the audit outcome and hand back the result with any warning.
    pub fn settle(self) -> (Result<O, cw_base::Error>, Option<AuditWarning>) {
        let warning = self.audit.settle();
        (self.result, warning)
    }

    /// The owned result and the pending record, for a surface that renders
    /// by value and settles the record afterwards.
    pub fn into_parts(self) -> (Result<O, cw_base::Error>, Pending) {
        (self.result, self.audit)
    }
}

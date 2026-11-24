//! The root help, grouped, and the check that keeps its summaries short.
//!
//! - [`grouped`] replaces clap's flat command list on the root help with the
//!   program's commands under headings, each with its one-line summary
//!   (the command's `about`), and keeps clap's usage, options and footer.
//! - [`check_summaries`] is the check a program's own test calls on its
//!   whole command tree: every command has a one-line summary no longer
//!   than the limit.

mod groups;
mod summary;

pub use groups::{Group, grouped};
pub use summary::{SUMMARY_LIMIT, check_summaries};

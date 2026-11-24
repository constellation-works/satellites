//! The chokepoint: the one path from any surface to a program's commands.
//!
//! A program describes itself once as an [`App`]: what it built at start,
//! its commands, which of them change data, how to run one, and what an
//! outcome acted on. Every surface (a command line, an MCP server, anything
//! added later) runs a command through [`exec()`], so starting the audit
//! record, running the command, and translating its error happen in one
//! place and no surface can skip them.

mod app;
mod context;
mod finished;
mod origin;

pub use app::App;
pub use context::{Ctx, Runtime};
pub use finished::Finished;
pub use origin::{Origin, Surface};

use crate::audit::Pending;

/// Run `command` of app `A` against `runtime`, for the client `origin`.
///
/// A command [`App::audit`] declares mutating gets its audit record before
/// it runs, so a panic is still logged as a failure; a read-only command
/// gets none, and nothing is created or locked for it. The app's typed
/// error crosses into [`cw_base::Error`] here, once, through its own
/// translator.
///
/// The result comes back with the pending record in a [`Finished`]: the
/// surface renders the result, then settles the record, so the line is
/// written after the output and a failure to write it reaches the surface
/// as a warning value.
pub fn exec<A: App>(
    runtime: &Runtime<A::State>,
    origin: &Origin,
    command: A::Command,
) -> Finished<A::Outcome> {
    let ctx = runtime.ctx(origin);
    let mut audit = match A::audit(&command) {
        Some(meta) => Pending::start(
            runtime.audit_log().clone(),
            runtime.audit_wait(),
            meta,
            ctx.now(),
            origin,
        ),
        None => Pending::none(),
    };
    let result = A::run(&ctx, command).map_err(Into::into);
    audit.mark(result.as_ref().map(A::target).map_err(cw_base::Error::code));
    Finished::new(result, audit)
}

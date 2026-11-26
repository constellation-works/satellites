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

use crate::audit::{AuditHandle, AuditMeta, Live, Pending};

/// Run `command` of app `A` against `runtime`, for the client `origin`.
///
/// A command [`App::audit`] declares mutating runs only after its started
/// line has been appended; [`App::subject`] is asked first, for that command
/// alone, so the line can name the record the command is aimed at. If preparing or appending the line fails, the
/// result contains `audit_start_failed` with a typed [`crate::Error`]
/// cause, or `audit_id_failed` if an ID could not be generated. A read-only
/// command creates no audit file and takes no audit lock. The app's typed
/// error crosses into [`cw_base::Error`] through its own translator.
///
/// An itemized command ([`crate::AuditMeta::itemized`]) is the exception:
/// nothing is written before it runs, and [`App::subject`] and
/// [`App::target`] are not called. It admits its items through
/// [`Ctx::audit`] inside its transaction, before its commit; if that
/// fails the command returns the error. After the reply each admitted item
/// settles as admitted, or as a failure with the command's code when it
/// returned an error. A command that fails having admitted nothing leaves one
/// standalone failure line.
///
/// The result comes back with the pending record in a [`Finished`]: the
/// surface renders the result, then settles the record, so the outcome is
/// written after the output and a failure to write it reaches the surface
/// as a warning value.
pub fn exec<A: App>(
    runtime: &Runtime<A::State>,
    origin: &Origin,
    command: A::Command,
) -> Finished<A::Outcome> {
    match A::audit(&command) {
        Some(meta) if meta.is_itemized() => exec_itemized::<A>(runtime, origin, meta, command),
        Some(meta) => exec_single::<A>(runtime, origin, meta, command),
        None => {
            let ctx = runtime.ctx_with(origin, AuditHandle::not_itemized());
            let result = A::run(&ctx, command).map_err(Into::into);
            Finished::new(result, Pending::none())
        }
    }
}

fn exec_single<A: App>(
    runtime: &Runtime<A::State>,
    origin: &Origin,
    meta: AuditMeta,
    command: A::Command,
) -> Finished<A::Outcome> {
    let ctx = runtime.ctx_with(origin, AuditHandle::not_itemized());
    let mut audit = match Pending::start(
        runtime.audit_log().clone(),
        runtime.audit_wait(),
        meta,
        // A read of the store, before the started line: nothing is
        // mutated before the line exists.
        A::subject(&ctx, &command),
        ctx.now(),
        origin,
    ) {
        Ok(audit) => audit,
        Err(error) => return Finished::new(Err(error), Pending::none()),
    };
    let result = A::run(&ctx, command).map_err(Into::into);
    // The target and the subject exist for the audit line alone: a read-only
    // command's outcome is never handed to `A::target`, nor the command to
    // `A::subject`.
    audit.mark(result.as_ref().map(A::target).map_err(cw_base::Error::code));
    Finished::new(result, audit)
}

fn exec_itemized<A: App>(
    runtime: &Runtime<A::State>,
    origin: &Origin,
    meta: AuditMeta,
    command: A::Command,
) -> Finished<A::Outcome> {
    let mut live = Live::new(
        runtime.audit_log().clone(),
        runtime.audit_wait(),
        meta,
        runtime.clock().now(),
        origin,
    );
    // Nothing is written before the run: the command admits its items inside
    // its transaction, and `A::subject` and `A::target` are never called.
    let result = {
        let ctx = runtime.ctx_with(origin, AuditHandle::itemized(&live));
        A::run(&ctx, command).map_err(Into::into)
    };
    live.mark(result.as_ref().err().map(cw_base::Error::code));
    Finished::new(result, Pending::itemized(live))
}

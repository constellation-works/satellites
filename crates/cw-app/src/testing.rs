//! A helper for an app's tests: nothing in the kit makes an itemized command
//! admit its items before it commits, because `cw-app` does not own the
//! command's transaction. [`exec_admitting`] fails the test when one
//! succeeds having written without admitting.

use crate::exec::{App, Finished, Origin, Runtime, exec};

/// Run `command` of app `A` through [`exec()`], and fail when it is itemized,
/// returned `Ok`, admitted no item, and `wrote` says it wrote.
///
/// `wrote` is called once, after the command has run, and answers whether the
/// command changed the store: compare a snapshot taken before the call with
/// the store after it, or read a counter the command bumps. A command that
/// is not itemized, one that failed, and one that admitted at least one item
/// pass through unchanged; so does an itemized command that changed nothing
/// (a no-op `Stop`), which must leave no line.
///
/// # Panics
///
/// When an itemized command returned `Ok` having written without admitting:
/// its committed write has no audit line.
pub fn exec_admitting<A: App>(
    runtime: &Runtime<A::State>,
    origin: &Origin,
    command: A::Command,
    wrote: impl FnOnce() -> bool,
) -> Finished<A::Outcome> {
    let itemized = A::audit(&command).is_some_and(|meta| meta.is_itemized());
    let finished = exec::<A>(runtime, origin, command);
    if itemized && finished.result().is_ok() && finished.admitted() == 0 && wrote() {
        panic!(
            "an itemized command returned Ok having written without admitting: \
             call `ctx.audit().admit(items)` after the last write and before the commit"
        );
    }
    finished
}

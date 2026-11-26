//! An itemized command: `tag <ids...>` tags several records in one
//! transaction and answers with one reply, yet leaves one audit pair per
//! record, and a refused id leaves a failure line beside the written ones.
//!
//! Run with `cargo run -p cw-app --example itemized`. It works in a temporary
//! directory and removes it on exit.

use cw_app::{App, AuditMeta, Cap, Ctx, Item, Origin, Runtime, Surface, exec};
use cw_base::{Error, Kind};
use std::io::Write as _;

struct Tag;

impl App for Tag {
    type State = ();
    type Command = Vec<&'static str>;
    type Outcome = usize;
    type Error = Error;

    fn audit(_: &Vec<&'static str>) -> Option<AuditMeta> {
        // Zero or many records: nothing is written before `run`.
        Some(AuditMeta::itemized("record tag"))
    }

    fn run(ctx: &Ctx<'_, ()>, ids: Vec<&'static str>) -> Result<usize, Error> {
        // ... the transaction's writes happen here ...
        let items: Vec<Item> = ids
            .iter()
            .map(|id| {
                if id.is_empty() {
                    Item::refused(None, "tag_empty")
                } else {
                    Item::wrote(Some((*id).to_owned()))
                }
            })
            .collect();
        // After the last write and before the commit: one append of every
        // started line. If it fails, return the error and roll back.
        ctx.audit().admit(items)?;
        // ... the transaction commits here ...
        if ids.is_empty() {
            return Err(Error::new(
                Kind::Usage,
                "tag_nothing",
                std::io::Error::other("no ids"),
            ));
        }
        Ok(ids.len())
    }

    // Never called for an itemized command: each item names its own subject.
    fn target(_: &usize) -> Option<String> {
        None
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let scratch = tempfile::tempdir()?;
    let runtime = Runtime::new(scratch.path().join("data"), ());
    let origin = Origin::new(Surface::CLI, None);

    let (result, warning) = exec::<Tag>(&runtime, &origin, vec!["n-1", "", "n-3"]).settle();
    assert_eq!(result?, 3);
    assert!(warning.is_none());

    let mut out = std::io::stdout().lock();
    for record in runtime.audit_log().read(Cap::bytes(1 << 16))? {
        let record = record?;
        writeln!(
            out,
            "{} {:?} target={:?} code={:?}",
            record.command, record.status, record.target, record.error_code
        )?;
    }
    Ok(())
}

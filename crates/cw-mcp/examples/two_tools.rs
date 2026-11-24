//! An MCP server with two tools over one in-memory tally: `tally_add`
//! changes it and is audited; `tally_show` only reads it. Both run through
//! the same chokepoint a command line would call, `cw_app::exec`.
//!
//! Run with `cargo run -p cw-mcp --example two_tools`, then write JSON-RPC
//! lines to its standard input, or point an MCP client at the command. It
//! audits to a temporary directory, removed on exit, and returns when its
//! input closes.

use cw_app::{App, AuditMeta, Ctx, Origin, Runtime, exec};
use cw_base::{Error, Kind};
use cw_mcp::{Access, ServerInfo, Tool, serve_stdio};
use serde_json::{Value, json};
use std::sync::Mutex;

/// What the program built at start: the tally.
struct State {
    tally: Mutex<u64>,
}

enum Command {
    Add(u64),
    Show,
}

/// The program's own typed error, translated once with its own codes.
#[derive(Debug, thiserror::Error)]
enum TallyError {
    #[error("cannot add zero")]
    Zero,
}

impl From<TallyError> for Error {
    fn from(error: TallyError) -> Self {
        match &error {
            TallyError::Zero => Error::new(Kind::Usage, "tally_zero", error),
        }
    }
}

struct Tally;

impl App for Tally {
    type State = State;
    type Command = Command;
    type Outcome = u64;
    type Error = TallyError;

    fn audit(command: &Command) -> Option<AuditMeta> {
        match command {
            Command::Add(_) => Some(AuditMeta::new("tally add")),
            Command::Show => None,
        }
    }

    fn run(ctx: &Ctx<'_, State>, command: Command) -> Result<u64, TallyError> {
        let mut tally = ctx
            .state()
            .tally
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match command {
            Command::Add(0) => Err(TallyError::Zero),
            Command::Add(n) => {
                *tally = tally.saturating_add(n);
                Ok(*tally)
            }
            Command::Show => Ok(*tally),
        }
    }

    fn target(_: &u64) -> Option<String> {
        Some("tally".to_owned())
    }
}

/// `tally_add`'s input: its schema is derived from this type.
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct AddInput {
    /// How much to add; zero is refused.
    n: u64,
}

/// `tally_show` takes no input.
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ShowInput {}

/// Run one command and render it as the JSON the client receives. The
/// audit line is written after the result is rendered.
fn run(runtime: &Runtime<State>, origin: &Origin, command: Command) -> Result<Value, Error> {
    let (result, warning) = exec::<Tally>(runtime, origin, command).settle();
    if let Some(warning) = warning {
        // Standard output carries the protocol; a warning goes to stderr.
        #[allow(clippy::print_stderr)]
        {
            eprintln!("warning: {warning}");
        }
    }
    result.map(|tally| json!({ "tally": tally }))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let runtime = Runtime::new(
        root.path(),
        State {
            tally: Mutex::new(0),
        },
    );
    let tools = vec![
        Tool::new::<AddInput>(
            "tally_add",
            "Add a number to the tally.",
            Access::Mutating,
            |runtime, origin, input| run(runtime, origin, Command::Add(input.n)),
        ),
        Tool::new::<ShowInput>(
            "tally_show",
            "Show the tally.",
            Access::ReadOnly,
            |runtime, origin, _| run(runtime, origin, Command::Show),
        ),
    ];
    let info = ServerInfo::new("tally", env!("CARGO_PKG_VERSION"), "tally_internal")
        .with_instructions("Add to the tally with tally_add; read it with tally_show.");
    serve_stdio(info, runtime, tools)?;
    Ok(())
}

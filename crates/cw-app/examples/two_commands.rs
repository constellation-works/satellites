//! A two-command program run through the chokepoint: `tally add <n>`
//! changes data and is audited; `tally show` only reads and is not.
//!
//! Run with `cargo run -p cw-app --example two_commands`. It works in a
//! temporary directory and removes it on exit.

use cw_app::env::{EnvVar, table};
use cw_app::{App, AuditMeta, Cap, Ctx, Env, Origin, Runtime, Surface, exec};
use cw_base::{Error, Kind};
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// The one variable this program reads beyond the kit's: declared once,
/// read through the declaration, and listed in the usage table.
const START: EnvVar =
    EnvVar::app("START", "The tally a fresh data directory starts from.").with_default("`0`");

/// What the program built at start: where its tally lives.
struct State {
    tally: PathBuf,
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
    #[error(transparent)]
    Fs(#[from] cw_fs::Error),
}

impl From<TallyError> for Error {
    fn from(error: TallyError) -> Self {
        match &error {
            TallyError::Zero => Error::new(Kind::Usage, "tally_zero", error),
            TallyError::Fs(fs) => Error::new(fs.kind(), "tally_io", error),
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
        let current = read_tally(&ctx.state().tally)?;
        match command {
            Command::Add(0) => Err(TallyError::Zero),
            Command::Add(n) => {
                let next = current.saturating_add(n);
                cw_fs::write_atomic(&ctx.state().tally, next.to_string().as_bytes())?;
                Ok(next)
            }
            Command::Show => Ok(current),
        }
    }

    fn target(_: &u64) -> Option<String> {
        Some("tally".to_owned())
    }
}

fn read_tally(path: &Path) -> Result<u64, TallyError> {
    if !cw_fs::inspect_file(path)? {
        return Ok(0);
    }
    let bytes = cw_fs::read(path, Cap::bytes(64))?;
    Ok(String::from_utf8_lossy(&bytes).trim().parse().unwrap_or(0))
}

/// The tally to start from: `None` when the variable is unset, an error
/// naming it when it is set to anything but a whole number.
fn start_from(env: &Env) -> Result<Option<u64>, String> {
    let app = env.app("tally");
    let Some(value) = app.get(&START) else {
        return Ok(None);
    };
    value
        .to_str()
        .and_then(|text| text.parse::<u64>().ok())
        .map(Some)
        .ok_or_else(|| {
            format!(
                "{} must be a whole number, not {value:?}",
                START.name_for(app.name())
            )
        })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // main reads the environment once, and refuses a value it cannot use
    // before it creates anything or prints anything.
    let env = Env::capture();
    let start = start_from(&env)?;

    // The data directory is a pure function of the captured value (here an
    // explicit root, as `--root` would be).
    let scratch = tempfile::tempdir()?;
    let root = env
        .app("tally")
        .data_dir(Some(&scratch.path().join("data")))?;
    cw_fs::create_private_dir(&root)?;

    let runtime = Runtime::new(
        &root,
        State {
            tally: root.join("tally"),
        },
    );
    let origin = Origin::new(Surface::CLI, env.user().map(str::to_owned));

    let mut out = std::io::stdout().lock();
    writeln!(out, "{}", table("tally", &[START]))?;

    // A fresh data directory starts from TALLY_START when it is set.
    if let Some(start) = start {
        cw_fs::write_atomic(&runtime.state().tally, start.to_string().as_bytes())?;
    }

    for command in [
        Command::Add(2),
        Command::Add(0),
        Command::Add(3),
        Command::Show,
    ] {
        // A mutating command runs only after its started line is synced.
        let finished = exec::<Tally>(&runtime, &origin, command);
        // Render the result, then settle the audit outcome.
        match finished.result() {
            Ok(total) => writeln!(out, "tally: {total}")?,
            Err(error) => writeln!(out, "error[{}]: {error}", error.code())?,
        }
        let (_, warning) = finished.settle();
        if let Some(warning) = warning {
            writeln!(out, "warning: {warning}")?;
        }
    }

    writeln!(out, "audited commands:")?;
    // The reader pairs lines by ID and shows Unknown for a start alone.
    for record in runtime.audit_log().read(Cap::bytes(1 << 20))? {
        let record = record?;
        writeln!(
            out,
            "  {} {:?} target={:?} code={:?}",
            record.command, record.status, record.target, record.error_code
        )?;
    }
    Ok(())
}

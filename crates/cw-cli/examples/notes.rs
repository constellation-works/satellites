//! A two-command program on cw-cli: `notes list` prints a fixed set of
//! notes as a table, tab-separated lines or JSON; `notes show <id>` prints
//! one, or fails with a hint.
//!
//! Try:
//!
//! ```text
//! cargo run -p cw-cli --example notes -- --help
//! cargo run -p cw-cli --example notes -- list
//! cargo run -p cw-cli --example notes -- list | cat
//! cargo run -p cw-cli --example notes -- list --format json
//! cargo run -p cw-cli --example notes -- show n9
//! NOTES_FORMAT=json cargo run -p cw-cli --example notes -- show n9
//! ```

use clap::{Arg, ArgMatches, Command, value_parser};
use cw_app::Env;
use cw_base::{Error, Kind};
use cw_cli::help::{Group, grouped};
use cw_cli::{Cell, Column, Exit, Format, Logging, Role, Sink, Table, Terminal};
use serde::Serialize;
use std::io::Write;
use std::process::ExitCode;

const APP: &str = "notes";

#[derive(Serialize)]
struct Note {
    id: &'static str,
    title: &'static str,
    pinned: bool,
    words: u32,
}

const NOTES: &[Note] = &[
    Note {
        id: "n1",
        title: "Groceries for the week",
        pinned: true,
        words: 42,
    },
    Note {
        id: "n2",
        title: "日本語のメモ",
        pinned: false,
        words: 7,
    },
];

/// The program's own typed error, translated once with its own codes.
#[derive(Debug, thiserror::Error)]
pub enum NotesError {
    /// No note has the id given.
    #[error("no note named {0:?}")]
    NotFound(String),
    /// Writing the output failed.
    #[error(transparent)]
    Output(#[from] cw_cli::Error),
}

impl From<NotesError> for Error {
    fn from(error: NotesError) -> Self {
        match &error {
            NotesError::NotFound(_) => Error::new(Kind::NotFound, "note_not_found", error),
            NotesError::Output(inner) => Error::new(inner.kind(), "output_failed", error),
        }
    }
}

/// The program's parser.
pub fn cli() -> Command {
    let format = Arg::new("format")
        .long("format")
        .global(true)
        .value_parser(value_parser!(Format))
        .help("Output format: auto, table or json [default: auto, or NOTES_FORMAT]");
    Command::new(APP)
        .about("Keep short notes")
        .arg(format)
        .subcommand_required(true)
        .subcommand(Command::new("list").about("List the notes"))
        .subcommand(
            Command::new("show").about("Show one note").arg(
                Arg::new("id")
                    .required(true)
                    .help("The note's id, such as n1"),
            ),
        )
}

const GROUPS: &[Group<'_>] = &[Group {
    heading: "Notes",
    commands: &["list", "show"],
}];

fn main() -> ExitCode {
    let env = Env::capture();
    Logging::from_env(&env, APP).init();
    // The help is grouped to the width of the terminal the decision found.
    let help_width = Terminal::detect(&env, APP, None).width();
    let matches = grouped(cli(), GROUPS, help_width).get_matches();
    let terminal = Terminal::detect(&env, APP, matches.get_one::<Format>("format").copied());
    let mut sink = Sink::stdio(terminal);
    let outcome = run(&mut sink, &matches).and_then(|()| sink.finish().map_err(NotesError::from));
    match outcome {
        Ok(()) => Exit::SUCCESS,
        Err(error) => {
            let hint = matches!(error, NotesError::NotFound(_))
                .then_some("run `notes list` to see the ids");
            sink.report(&Error::from(error), hint)
        }
    }
    .into()
}

/// Run the command `matches` names, writing through `sink`.
pub fn run<O: Write, E: Write>(
    sink: &mut Sink<O, E>,
    matches: &ArgMatches,
) -> Result<(), NotesError> {
    match matches.subcommand() {
        Some(("show", args)) => {
            let id = args.get_one::<String>("id").map_or("", String::as_str);
            let note = NOTES
                .iter()
                .find(|note| note.id == id)
                .ok_or_else(|| NotesError::NotFound(id.to_owned()))?;
            sink.emit_text(note, |note| format!("{}\n{} words", note.title, note.words))?;
        }
        _ => {
            sink.emit(NOTES, table)?;
            // A reader that went away gets silence, on stderr too (STD-01
            // R13): the count is for a human still reading.
            if !sink.is_closed() {
                sink.notice(&format!("{} notes", NOTES.len()));
            }
        }
    }
    Ok(())
}

/// The human view of the list, derived from the same payload as the JSON.
fn table(notes: &[Note]) -> Table {
    let mut table = Table::new([
        Column::new("ID"),
        Column::new("TITLE").truncatable(),
        Column::new("WORDS").right(),
        Column::new("PINNED"),
    ]);
    for note in notes {
        let pinned = if note.pinned {
            Cell::new("yes").role(Role::Active)
        } else {
            Cell::absent()
        };
        table.push([
            Cell::new(note.id),
            Cell::new(note.title),
            Cell::new(note.words.to_string()),
            pinned,
        ]);
    }
    table
}

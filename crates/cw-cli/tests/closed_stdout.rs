//! A closed stdout ends the writes, not the command, and the exit is 0.

use cw_app::Env;
use cw_cli::{Exit, Format, Probe, Sink, Table, Terminal};
use std::io::{self, Write};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A reader that has gone away: every write is a broken pipe.
struct Closed {
    attempts: usize,
}

impl Write for Closed {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        self.attempts += 1;
        Err(io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::ErrorKind::BrokenPipe.into())
    }
}

#[test]
fn a_closed_stdout_latches_shut_and_the_command_exits_zero() -> TestResult {
    let terminal = Terminal::decide(&Env::default(), "notes", Probe::piped(), Some(Format::Json));
    let mut sink = Sink::new(terminal, Closed { attempts: 0 }, Vec::new());

    // A command writes, then commits: neither write may stop it early.
    let mut done = Vec::new();
    sink.emit(&["first"], |_| Table::default())?;
    assert!(sink.is_closed());
    sink.emit(&["second"], |_| Table::default())?;
    done.push("committed");

    let exit = match sink.finish() {
        Ok(()) => Exit::SUCCESS,
        Err(error) => panic!("a closed reader is not a failure: {error}"),
    };
    let (out, err) = sink.into_parts();
    assert_eq!(done, ["committed"]);
    assert_eq!(exit.code(), 0);
    assert_eq!(out.attempts, 1, "nothing is written after the latch");
    assert!(err.is_empty(), "nothing on stderr");
    Ok(())
}

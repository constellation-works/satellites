//! A closed stdout ends the writes, not the command, and the exit is 0.

use cw_app::Env;
use cw_cli::{Error, Exit, Format, Probe, Sink, Table, Terminal};
use std::io::{self, Write};

/// The example program, so its own command path runs against a closed reader.
#[expect(
    dead_code,
    reason = "this test runs the example's list path, not its main"
)]
#[path = "../examples/notes.rs"]
mod notes;

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

/// A writer that takes nothing, or whose first flush is interrupted.
#[derive(Default)]
struct Stubborn {
    takes_nothing: bool,
    interrupted: bool,
    flushed: bool,
}

impl Write for Stubborn {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(if self.takes_nothing { 0 } else { buf.len() })
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.interrupted {
            self.interrupted = false;
            return Err(io::ErrorKind::Interrupted.into());
        }
        self.flushed = true;
        Ok(())
    }
}

#[test]
fn a_write_that_takes_nothing_fails_finish_and_an_interrupted_flush_is_retried() -> TestResult {
    let terminal = Terminal::decide(&Env::default(), "notes", Probe::piped(), Some(Format::Json));

    let takes_nothing = Stubborn {
        takes_nothing: true,
        ..Stubborn::default()
    };
    let mut sink = Sink::new(terminal, takes_nothing, Vec::new());
    sink.emit(&["payload"], |_| Table::default())?;
    match sink.finish() {
        Err(Error::Output(error)) => assert_eq!(error.kind(), io::ErrorKind::WriteZero),
        other => panic!("a lost payload must fail finish: {other:?}"),
    }

    let interrupted = Stubborn {
        interrupted: true,
        ..Stubborn::default()
    };
    let mut sink = Sink::new(terminal, interrupted, Vec::new());
    sink.emit(&["payload"], |_| Table::default())?;
    sink.finish()?;
    assert!(
        sink.into_parts().0.flushed,
        "the interrupted flush is retried"
    );
    Ok(())
}

#[test]
fn the_example_list_is_silent_on_stderr_after_its_reader_goes_away() -> TestResult {
    let terminal = Terminal::decide(&Env::default(), "notes", Probe::piped(), None);
    let mut sink = Sink::new(terminal, Closed { attempts: 0 }, Vec::new());
    let matches = notes::cli().try_get_matches_from(["notes", "list"])?;
    let outcome = notes::run(&mut sink, &matches);
    assert!(outcome.is_ok(), "{outcome:?}");
    sink.finish()?;
    let (_, err) = sink.into_parts();
    assert_eq!(String::from_utf8(err)?, "", "nothing on stderr");
    Ok(())
}

//! Run a shell command within bounds and show how it ended: once to a clean
//! exit, once past its deadline, once past a kill cap.
//!
//! Run with `cargo run -p cw-supervise --example run_with_deadline`. Unix
//! only; elsewhere every run returns `Error::Unsupported`.

use cw_supervise::{Cap, End, Outcome, Run, env};
use std::process::Command;
use std::time::Duration;

fn shell(script: &str) -> Command {
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg(script);
    env::allow(&mut command, &["PATH", "HOME", "LANG"]);
    command
}

fn report(out: &mut impl std::io::Write, label: &str, outcome: &Outcome) -> std::io::Result<()> {
    let end = match &outcome.end {
        End::Exited(status) => format!("exited ({status})"),
        End::TimedOut => String::from("timed out"),
        End::OutputLimit => String::from("stopped at the output cap"),
    };
    writeln!(
        out,
        "{label}: {end}; stdout {} bytes{}, stderr {:?}",
        outcome.stdout.bytes.len(),
        if outcome.stdout.truncated {
            " (truncated)"
        } else {
            ""
        },
        String::from_utf8_lossy(&outcome.stderr.bytes).trim_end(),
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut out = std::io::stdout().lock();

    let clean = Run::new(
        shell("echo hello; echo careful >&2"),
        Duration::from_secs(5),
    )
    .run()?;
    report(&mut out, "clean", &clean)?;

    let hung = Run::new(
        shell("echo stuck >&2; sleep 30"),
        Duration::from_millis(300),
    )
    .grace(Duration::from_millis(200))
    .run()?;
    report(&mut out, "hung", &hung)?;

    let loud = Run::new(shell("yes"), Duration::from_secs(5))
        .stdout(Cap::kill(1024))
        .run()?;
    report(&mut out, "loud", &loud)?;
    Ok(())
}

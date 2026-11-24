//! Run a child in an isolated environment and check its output against a
//! golden file, as an integration test would.
//!
//! ```text
//! cargo run -p cw-testkit --example isolated_child
//! ```

use std::io::Write as _;
use std::process::ExitCode;

use cw_testkit::{Goldens, TempRoot};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let _ = writeln!(std::io::stderr(), "isolated_child: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let root = TempRoot::new("EXAMPLE")?;

    // The child sees HOME, PATH, EXAMPLE_ROOT and what we add, nothing else.
    let out = root
        .command("sh")
        .args([
            "-c",
            "echo \"greeting=$GREETING home_is_set=${HOME:+yes} user=${USER:-unset}\"",
        ])
        .env("GREETING", "hello")
        .output()?;
    let text = String::from_utf8(out.stdout)?;

    // A golden would normally be checked in under tests/goldens/; here it
    // lives in the temp root so the example leaves nothing behind.
    let goldens = Goldens::new(root.elsewhere("goldens"));
    std::fs::create_dir_all(goldens.dir())?;
    std::fs::write(
        goldens.dir().join("greeting.txt"),
        "greeting=hello home_is_set=yes user=unset\n",
    )?;
    goldens.check("greeting.txt", &text)?;

    writeln!(std::io::stdout(), "{}", text.trim_end())?;
    Ok(())
}

//! `serve_stdio` on a real standard input: it returns when the input
//! closes. The test runs this binary again as the server, with piped
//! standard input and output.

#![expect(
    clippy::disallowed_methods,
    reason = "stdio fixture: starts the server binary and keeps its pipes open for the session, which the bounded runner cannot do"
)]

#[expect(
    dead_code,
    reason = "this test binary uses only part of the shared fixture"
)]
mod fixture;

use cw_app::Runtime;
use cw_mcp::{ServerInfo, serve_stdio};
use serde_json::{Value, json};
use std::io::{BufRead as _, BufReader, Write as _};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long the server has to answer, and then to exit.
const DEADLINE: Duration = Duration::from_secs(20);

/// The server side: run only as the child the test below starts.
#[test]
#[ignore = "the server side of serve_stdio_returns_when_its_input_closes"]
fn stdio_server() {
    let root = tempfile::tempdir().unwrap();
    let info = ServerInfo::new("fixture", "1.2.3", fixture::PANIC_CODE);
    let runtime = Runtime::new(root.path(), fixture::Calls::default());
    serve_stdio(info, runtime, fixture::tools()).expect("serve_stdio returns Ok");
}

/// The server child, killed and reaped when dropped, so a failing test
/// leaves no process behind.
struct Reaped(Child);

impl Drop for Reaped {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn serve_stdio_returns_when_its_input_closes() {
    // The child runs in a root this test owns, with HOME there too.
    let root = tempfile::tempdir().unwrap();
    let mut child = Reaped(
        Command::new(std::env::current_exe().unwrap())
            .args(["stdio_server", "--exact", "--ignored", "--test-threads=1"])
            .current_dir(root.path())
            .env("HOME", root.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );

    let mut stdin = child.0.stdin.take().unwrap();
    let initialize = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "stdio-client", "version": "0"},
        },
    });
    writeln!(stdin, "{initialize}").unwrap();
    stdin.flush().unwrap();

    // The test harness prints its own text on stdout too, and the server's
    // first line follows the harness's `test stdio_server ... ` on the same
    // line; the server's JSON starts at the first brace.
    let stdout = child.0.stdout.take().unwrap();
    let (lines, received) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let json = line.find('{').map(|at| &line[at..]);
            if let Some(Ok(value)) = json.map(serde_json::from_str::<Value>) {
                let _ = lines.send(value);
            }
        }
    });
    let reply = received.recv_timeout(DEADLINE).expect("initialize answers");
    assert_eq!(reply["result"]["serverInfo"]["name"], "fixture", "{reply}");

    drop(stdin);
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        // The guard kills and reaps the child as the panic unwinds.
        if started.elapsed() > DEADLINE {
            panic!("serve_stdio did not return within {DEADLINE:?} of its input closing");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success(), "the server exited with {status}");
}

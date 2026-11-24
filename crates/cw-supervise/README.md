# cw-supervise

Run a child process within bounds: in its own process group, with a required deadline, a
polite-then-forced stop, a sweep of the whole group on every exit path, and capped output.
Synchronous, on std threads; system calls through `rustix`, no `unsafe`.

```rust
use cw_supervise::{Cap, End, Run, env};
use std::process::Command;
use std::time::Duration;

let mut command = Command::new("git");
command.args(["status", "--porcelain"]);
env::allow(&mut command, &["PATH", "HOME", "LANG"]);

let outcome = Run::new(command, Duration::from_secs(30)) // the deadline is required
    .stdin(Vec::new())                                   // default: none, the child reads EOF
    .stdout(Cap::truncate(1 << 20))                      // or Cap::kill(n)
    .stderr(Cap::truncate(64 << 10))
    .grace(Duration::from_secs(1))                       // default 1 s
    .run()?;                                             // Err: Spawn { program, source } | Unsupported
```

`Run::new(..).combined(Cap::kill(n))` captures both streams as one interleaved log instead.

| Type | Holds |
|---|---|
| `Outcome` | `end`, the `stdout` and `stderr` captures, and `swept` (cleanup reached the group and reaped the leader) |
| `End` | `Exited(ExitStatus)`, `TimedOut` or `OutputLimit` |
| `Captured` | `bytes`, `truncated` (more than the cap was written), `complete` (read to its end) |
| `Cap` | `truncate(n)`: keep `n` bytes, drain the rest; `kill(n)`: keep `n` bytes, stop the group |

## Guarantees

- **The crate isolates the child.** It sets the new process group and all three stdio handles
  on the caller's command, so a caller cannot forget them. Program, arguments, environment,
  working directory and inherited descriptors stay the caller's; nothing is closed.
- **At the deadline:** `SIGTERM` to the group, up to the grace (ending early once the leader
  exits), `SIGKILL` to the group regardless, then the reap. The result is `End::TimedOut`
  with what was captured.
- **Every path ends with a sweep**, a clean exit included: `SIGKILL` to the group while its
  leader is unreaped, then the reap, waited for at most 2 s. No signal is sent after the
  reap. A drop guard does the same on a panic or an early return.
- **Output is never lost silently.** After the sweep the readers get up to 2 s. A pipe still
  held then (by a process that left the group) is returned with what was read and
  `complete: false`, and the real exit status is kept. An interrupted read is retried.
- **Only a failure to start is an error**, typed, with the program and the `io::Error`, so
  "not installed" (`ErrorKind::NotFound`) is distinguishable. A timeout and an output
  overflow are outcomes.

## The environment

`env::allow(&mut command, &["PATH", "HOME"])` clears the environment and copies each named
variable that is set. `env::Allow` adds a prefix to pass and a prefix never to pass (a
tool's own override variables), and `apply_from` takes a snapshot instead of this process's
environment. The crate ships no list of names.

## Limits

- A descendant that calls `setsid` or daemonizes is not swept; stopped descendants are not
  continued before the sweep.
- Such a descendant can keep a reader (or stdin writer) thread blocked, with its pipe, after
  the run returns, until it closes the pipe. The crate is sized for command-line tools.
- The grace after `SIGTERM` ends as soon as the leader exits, deliberately, as the design
  decided.
- When cleanup fails or cannot be verified, the run returns with `Outcome::swept` false
  rather than waiting without bound; a leader left unreaped then stays a zombie until the
  caller exits. On macOS `swept` is also false when the leader exited before the sweep
  (the kernel refuses a signal to a group of zombies as it does one it may not send).
- A deadline or grace longer than about ten years is treated as ten years.
- Precondition: the calling process reaps its children only through this crate: no other
  waiter, and `SIGCHLD` is neither ignored nor set with `SA_NOCLDWAIT`.
- Linux and macOS only. On any other platform every run returns `Error::Unsupported`; there
  is no weaker fallback.

## Errors

`cw_supervise::Error` stands alone, with no kind and no code: an adopter maps `Spawn` and
`Unsupported` to its own `Kind` and code in its `From` impl. It is not `#[non_exhaustive]`.

## Example

```sh
cargo run -p cw-supervise --example run_with_deadline
```

## License

MIT.

# cw-testkit

Test isolation for command-line tools: a test that runs a program must not read or write the
machine it runs on. `cw-testkit` is a dev-dependency that gives each test:

- **`TempRoot`** — a private (`0700`) temporary directory holding a `home/`, a `bin/` for fake
  programs and the application's data root (`root/`, left for the program to create). It is
  parameterised by the application's environment prefix: `TempRoot::new("MYAPP")` points
  `MYAPP_ROOT` at it. The root, `home/` and `bin/` are set to `0700` explicitly after they are
  made, through a handle opened without following links (`fchmod`), so a restrictive umask (`0277`, say) cannot leave them narrower. A root whose `bin/`
  path contains the `PATH` separator (`:` on Unix, which a `TMPDIR` can carry) is refused with
  an error naming the path, because it would split the child's `PATH`. Drop removes it
  best-effort and ignores a failure; `close()` removes it and returns the error.
- **`TempRoot::command`** — a `std::process::Command` run in the temp root, whose environment
  is cleared, then given only `HOME`, `PATH` (the temp root's `bin/`, then `/usr/bin:/bin`),
  `<APP>_ROOT` and `GIT_CEILING_DIRECTORIES` (the temp root's parent, so git never discovers a
  repository above the fixture, even when the system temporary directory is inside one, unless
  that parent's path contains a `:`, which git cannot hold in the variable). The
  test adds whatever else it names; nothing else is inherited, so a developer's exported
  variables, colour settings, real data root or surrounding checkout never reach the program
  under test.
- **`Goldens`** — compares output with a file under the test's `goldens/` folder. With
  `UPDATE_GOLDENS=1` it rewrites the file instead. A mismatch is an error naming the file and
  the variable, so a test can collect every stale golden before failing. A golden is never read
  or written through a symbolic link, the goldens folder itself included, so an update cannot
  write outside the folder.
  `redact_timestamps` replaces RFC 3339 timestamps with `<timestamp>` first, so the clock
  never makes a golden stale.

The crate is synchronous and has no unsafe code. It stands alone: it depends on no other kit
crate, so its `thiserror` `Error` has no `kind()`; its errors only ever end a test.

## Usage

```toml
[dev-dependencies]
cw-testkit = "0.1"
```

```rust,no_run
use cw_testkit::{Goldens, TempRoot, redact_timestamps};

#[test]
fn list_matches_its_golden() {
    let root = TempRoot::new("MYAPP").expect("temp root");
    let out = root
        .command(env!("CARGO_BIN_EXE_myapp"))
        .args(["list"])
        .env("MYAPP_LOG", "off")
        .output()
        .expect("run myapp");
    let text = String::from_utf8(out.stdout).expect("utf-8");

    let goldens = Goldens::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/goldens"));
    goldens.assert("list.txt", &redact_timestamps(&text));
}
```

After an intended output change, regenerate and review the diff as a contract change:

```sh
UPDATE_GOLDENS=1 cargo test
```

`examples/isolated_child.rs` runs a child in an isolated environment and checks its output
against a golden: `cargo run -p cw-testkit --example isolated_child`.

## The hostile-environment script

`scripts/hostile-env-test.sh` in this repository runs a whole suite under a read-only decoy
`HOME` and `XDG_*` tree, the system search paths included (with a git config whose hooks
leave a marker), and a `TMPDIR` whose path has a space and a non-ASCII letter. It then checks
that the suite passed, no hook ran, the home is unchanged and `TMPDIR` is empty. A plain read
of the decoys is not detected; it only finds decoys instead of the developer's files. Copy it into a project and pass the suite's command:

```sh
scripts/hostile-env-test.sh cargo test --workspace --locked
```

## License

MIT.

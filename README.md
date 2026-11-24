# satellites

General-purpose Rust crates spun out of [Orbit](https://github.com/constellation-works/orbit)
and the rest of the constellation-works tools, packaged for anyone to use. Each crate stands on
its own: no dependency on Orbit, its own docs and examples, and semver from its first release.

## Crates

| Crate | What it does | Status |
|---|---|---|
| [cw-base](crates/cw-base) | One error type that keeps a unit's typed error whole, with its kind and code; a clock a test can pin; RFC 3339 timestamps | 0.1.0, unreleased |
| [cw-fs](crates/cw-fs) | Durable, owner-only file access and one advisory lock with a deadline | 0.1.0, unreleased |
| [cw-testkit](crates/cw-testkit) | Test isolation for command-line tools: private temp roots, a child command with a cleared environment, and a golden-file harness | 0.1.0, unreleased |
| [cw-supervise](crates/cw-supervise) | Run a child process in its own group with a required deadline, a polite-then-forced stop, a sweep on every exit path and capped output | 0.1.0, unreleased |
| — | A cross-platform process sandbox (Landlock, Bubblewrap, Seatbelt) | In progress |

## Development

`make ci` runs everything CI runs (fmt, clippy, cargo-deny, tests, doctests) on Linux and macOS.
`make ci-fast` is the pre-handoff gate and does not compile.

## License

MIT. See [LICENSE](LICENSE).

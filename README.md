# satellites

General-purpose Rust crates spun out of [Orbit](https://github.com/constellation-works/orbit)
and the rest of the constellation-works tools, packaged for anyone to use. Each crate stands on
its own: no dependency on Orbit, its own docs and examples, and semver from its first release.

## Crates

| Crate | What it does | Status |
|---|---|---|
| — | — | The first crate, a cross-platform process sandbox (Landlock, Bubblewrap, Seatbelt), is in progress. |

## Development

`make ci` runs everything CI runs (fmt, clippy, cargo-deny, tests, doctests) on Linux and macOS.
`make ci-fast` is the pre-handoff gate and does not compile.

## License

MIT. See [LICENSE](LICENSE).

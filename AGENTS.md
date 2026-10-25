# satellites — agent guide

Loaded as both `AGENTS.md` and `CLAUDE.md` (a symlink). satellites holds general-purpose Rust
crates spun out of Orbit and other constellation-works tools, published for anyone to use.

## Rules

- Work only on authorized scope. Daniel owns releases: never run `cargo publish` (other than
  `--dry-run`), bump a published version, tag or trigger a release.
- A crate here must not depend on any Orbit (`orbit-*`) or other constellation-internal crate.
  Code that only makes sense inside Orbit stays in Orbit; Orbit adapts its own types onto
  these crates.
- Every crate is public API with semver: document every public item (`missing_docs` is
  denied), ship a README and at least one example, and treat any change to a public type or
  behavior as a contract change.
- New crate: `crates/<name>/`, `[lints] workspace = true`, inherit `[workspace.package]`, add a
  row to the README's Crates table in the same change.
- Update affected docs in the same change as the code.
- A fix comes with a regression test through the public API, seen failing before the fix.
- Tests never touch real host state: temp roots only.
- Report commands and outcomes at handoff — passed, failed, not run — never "tested".

## Branching

- `agent-main` — dev integration; commit directly. `main` — releases only.

## Gates

`make ci-fast` (fmt, standards-check) and `make ci-lint` must pass before a task moves to
review. `make ci` (everything, including tests and doctests) runs in CI on Linux and macOS.

<!-- constellation-standards:begin -->
<!-- Managed by the constellation's operations/scripts/sync-standards.sh; edits inside this block are overwritten. -->
## Constellation standards

This repository adopts these constellation standards, vendored read-only in `docs/standards/` (each a directory: `STD-nn.md` holds the rules, `why.md` and `checks.md` the reasons and gates):

- `STD-02@3` — [docs/standards/STD-02-rust-architecture-and-errors/STD-02.md](docs/standards/STD-02-rust-architecture-and-errors/STD-02.md)
- `STD-03@2` — [docs/standards/STD-03-concurrency-and-process-safety/STD-03.md](docs/standards/STD-03-concurrency-and-process-safety/STD-03.md)
- `STD-04@2` — [docs/standards/STD-04-testing-and-verification/STD-04.md](docs/standards/STD-04-testing-and-verification/STD-04.md)
- `STD-05@1` — [docs/standards/STD-05-security-boundaries/STD-05.md](docs/standards/STD-05-security-boundaries/STD-05.md)

Follow them; they are normative. To deviate from a rule, record a decision in `docs/design/<feature>/4_decisions.md` citing `STD-nn@<version> §Rn`; never edit `docs/standards/` (`sh docs/standards/check.sh` enforces this).
Reviewers check every change against the adopted standards and report violations as `STD-nn §Rn` with file:line evidence.
<!-- constellation-standards:end -->

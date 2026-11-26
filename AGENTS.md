# satellites — agent guide

Loaded as both `AGENTS.md` and `CLAUDE.md` (a symlink). satellites holds general-purpose Rust
crates, published for anyone to use.

## Rules

- Work only on authorized scope. Daniel owns releases: never run `cargo publish` (other than
  `--dry-run`), bump a published version, tag or trigger a release.
- A crate here must not depend on any Orbit (`orbit-*`) or other constellation-internal crate.
  Code that only makes sense inside Orbit stays in Orbit; Orbit adapts its own types onto
  these crates.
- Every crate is public API with semver: document every public item, ship a README and at
  least one example, and treat any change to a public type or behavior as a contract change.
- New crate: `crates/<name>/`, `[lints] workspace = true`, inherit `[workspace.package]`
  ([STD-02](docs/standards/STD-02-rust-architecture-and-errors/STD-02.md) §R9, §R22), add a
  row to the README's Crates table in the same change.
- Update affected docs in the same change as the code. Edit this file only when a rule, a
  command or a pointer in it changes; describing a feature goes in that feature's docs.

## Branching

- `agent-main` — dev integration; commit directly. `main` — releases only.

## Gates

- `make ci-fast` and `make ci-lint` must pass before a task moves to review.
- `make ci` runs in CI on Linux and macOS.

## Tests

- Regression tests through the public API, and the command outcomes a handoff lists:
  [STD-04](docs/standards/STD-04-testing-and-verification/STD-04.md) §R1, §R2, §R16.
- Host state in tests: [STD-03](docs/standards/STD-03-concurrency-and-process-safety/STD-03.md) §R20.

<!-- constellation-standards:begin -->
<!-- Managed by the constellation's operations/scripts/sync-standards.sh; edits inside this block are overwritten. -->
## Constellation standards

This repository adopts these constellation standards, vendored read-only in `docs/standards/` (each a directory: `STD-nn.md` holds the rules, `why.md` and `checks.md` the reasons and gates):

- `STD-02@8` — [docs/standards/STD-02-rust-architecture-and-errors/STD-02.md](docs/standards/STD-02-rust-architecture-and-errors/STD-02.md): Rust architecture and errors — layering, visibility, typed errors, failure semantics, idiom and naming, logging, and…
- `STD-03@2` — [docs/standards/STD-03-concurrency-and-process-safety/STD-03.md](docs/standards/STD-03-concurrency-and-process-safety/STD-03.md): Concurrency and process safety — locks, channels, durable writes, state evolution, subprocess lifecycles, destructive…
- `STD-04@2` — [docs/standards/STD-04-testing-and-verification/STD-04.md](docs/standards/STD-04-testing-and-verification/STD-04.md): Testing and verification — test design, hermetic tests, honest gates, docs as contracts, and evidence-based…
- `STD-05@1` — [docs/standards/STD-05-security-boundaries/STD-05.md](docs/standards/STD-05-security-boundaries/STD-05.md): Security boundaries — authority, filesystem containment, child environments, redaction, local network surfaces, supply…

Follow them; they are normative. To deviate from a rule, record a decision in `docs/design/<feature>/4_decisions.md` citing `STD-nn@<version> §Rn`; never edit `docs/standards/` (`sh docs/standards/check.sh` enforces this).
Reviewers check every change against the adopted standards and report violations as `STD-nn §Rn` with file:line evidence.
<!-- constellation-standards:end -->

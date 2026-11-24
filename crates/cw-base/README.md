# cw-base

The base every unit of a Rust command-line program builds on: one error type that keeps a
unit's typed error whole, and a clock a test can pin. No I/O and no environment.

## Error and Kind

Each unit keeps its own typed `thiserror` enum and writes one `From` impl beside it that
states the failure's `Kind` and its stable code. Everything above the unit carries
`cw_base::Error`:

- `kind()` says what the caller can do about the failure: `Usage`, `NotFound`, `Refused`,
  `Busy`, `Corrupt`, `Incompatible`, `Config`, `Io` or `Internal`. A command line exits 2
  for `Usage` and 1 for the rest; a tool server marks `Busy` retryable.
- `code()` is the project's own stable `snake_case` code. Kit crates have kinds and no
  codes: the project that wraps their errors names them.
- `downcast_ref::<T>()` gives the typed error back, whole, to a caller that knows the unit.
- `Display` and `source()` are the typed error's own.

`Kind` is not `#[non_exhaustive]`: a new kind is meant to break every match that decides
on it.

```rust
use cw_base::{Error, Kind};

#[derive(Debug, thiserror::Error)]
enum StoreError {
    #[error("no note named {0}")]
    Missing(String),
}

impl From<StoreError> for Error {
    fn from(error: StoreError) -> Self {
        match &error {
            StoreError::Missing(_) => Error::new(Kind::NotFound, "note_not_found", error),
        }
    }
}
```

## Time

`Clock::system()` reads the operating system's clock; `Clock::fixed(at)` always answers
`at`, so a test that hands it down pins every stamp. Timestamps are RFC 3339 in UTC:
`format_rfc3339` and `parse_rfc3339`.

## Example

```sh
cargo run -p cw-base --example unit_error
```

## License

MIT.

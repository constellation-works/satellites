//! `tracing` to stderr, at the level `<APP>_LOG` names.
//!
//! Logging is a side channel, so it never fails a command: a filter that does
//! not parse is replaced by the default, `warn`, and the reason is kept as a
//! warning for the program to show ([`Logging::warning`]). [`Logging::init`]
//! writes the warning to stderr itself and installs the subscriber; when one
//! is already installed it leaves that one in place.

use cw_app::Env;
use std::io;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;

/// The level used when `<APP>_LOG` is unset, empty or does not parse.
pub const DEFAULT_LEVEL: &str = "warn";

/// A program's logging, resolved from the captured environment.
#[derive(Debug)]
pub struct Logging {
    filter: EnvFilter,
    warning: Option<String>,
}

impl Logging {
    /// Resolve the filter from `<APP>_LOG` in `env`, for the program named
    /// `app`. Unset, empty, or holding no directive (`,`) means
    /// [`DEFAULT_LEVEL`]. Never fails: a value that is not UTF-8 or does not parse
    /// leaves the default in place and a [`Logging::warning`].
    #[must_use]
    pub fn from_env(env: &Env, app: &str) -> Self {
        let app = env.app(app);
        let name = app.var_name("LOG");
        let Some(value) = app.log() else {
            return Self::default_level(None);
        };
        let Some(spec) = value.to_str() else {
            return Self::default_level(Some(format!(
                "ignoring {name}: not valid UTF-8; logging at {DEFAULT_LEVEL}"
            )));
        };
        // A value with no directive in it (`,`) parses to a filter that
        // passes nothing, not even a warning: it means the default.
        if spec.split(',').all(|directive| directive.trim().is_empty()) {
            return Self::default_level(None);
        }
        match EnvFilter::try_new(spec) {
            Ok(filter) => Self {
                filter,
                warning: None,
            },
            Err(error) => Self::default_level(Some(format!(
                "ignoring {name}={spec:?} ({error}); logging at {DEFAULT_LEVEL}"
            ))),
        }
    }

    fn default_level(warning: Option<String>) -> Self {
        Self {
            filter: EnvFilter::new(DEFAULT_LEVEL),
            warning,
        }
    }

    /// Why the variable was ignored, when it was.
    #[must_use]
    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }

    /// The subscriber, writing plain lines with no time or target to
    /// `writer`: the process's stderr in [`Logging::init`], a buffer in a
    /// test (with `tracing::subscriber::with_default`).
    pub fn subscriber<W>(self, writer: W) -> impl tracing::Subscriber + Send + Sync + 'static
    where
        W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
    {
        tracing_subscriber::fmt()
            .with_writer(writer)
            .with_env_filter(self.filter)
            .with_target(false)
            .with_ansi(false)
            .without_time()
            .finish()
    }

    /// Write the warning, if any, to stderr, and install the subscriber on
    /// stderr for the whole process. A subscriber already installed is left
    /// in place. Never fails.
    pub fn init(self) {
        if let Some(warning) = &self.warning {
            use std::io::Write as _;
            let _ = writeln!(io::stderr(), "warning: {warning}");
        }
        let _ = tracing::subscriber::set_global_default(self.subscriber(io::stderr));
    }
}

//! Logging: `<APP>_LOG` sets the level, `warn` by default, never fatal.

use cw_app::Env;
use cw_cli::Logging;
use std::io;
use std::sync::{Arc, Mutex, PoisonError};

/// A writer the test reads back: what the subscriber sends to stderr.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Log one line at each level under `vars`, and return what was written.
fn logged(vars: &[(&str, &str)]) -> (String, Option<String>) {
    let logging = Logging::from_env(&Env::from_vars(vars.iter().copied()), "notes");
    let warning = logging.warning().map(str::to_owned);
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = logging.subscriber(move || writer.clone());
    tracing::subscriber::with_default(subscriber, || {
        tracing::debug!("a debug line");
        tracing::info!("an info line");
        tracing::warn!("a warn line");
    });
    let bytes = captured
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    (String::from_utf8_lossy(&bytes).into_owned(), warning)
}

#[test]
fn the_level_comes_from_the_app_log_variable() {
    let (out, warning) = logged(&[("NOTES_LOG", "info")]);
    assert!(out.contains("an info line"), "{out}");
    assert!(out.contains("a warn line"), "{out}");
    assert!(!out.contains("a debug line"), "{out}");
    assert_eq!(warning, None);
}

#[test]
fn the_level_defaults_to_warn() {
    let (out, warning) = logged(&[]);
    assert!(out.contains("a warn line"), "{out}");
    assert!(!out.contains("an info line"), "{out}");
    assert_eq!(warning, None);
}

#[test]
fn a_bad_level_falls_back_to_warn_with_a_warning() {
    let (out, warning) = logged(&[("NOTES_LOG", "notes=[[[")]);
    assert!(out.contains("a warn line"), "{out}");
    assert!(!out.contains("an info line"), "{out}");
    assert!(warning.is_some_and(|w| w.contains("NOTES_LOG")));
}

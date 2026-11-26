//! The variables the kit itself reads, declared once for every program.
//!
//! Each name below is the only place in this crate that spells it:
//! [`super::Env`] and [`super::AppEnv`] read through these declarations.

use super::var::EnvVar;

/// The data directory, under the program's prefix.
pub const ROOT: EnvVar = EnvVar::app("ROOT", "The data directory.")
    .with_default("`$HOME/.<app>`")
    .with_flag("--root");

/// The output format, under the program's prefix.
pub const FORMAT: EnvVar = EnvVar::app("FORMAT", "The output format.");

/// The log filter, under the program's prefix.
pub const LOG: EnvVar = EnvVar::app("LOG", "The log filter.");

/// Output without color, the `no-color.org` convention.
pub const NO_COLOR: EnvVar = EnvVar::global("NO_COLOR", "Any non-empty value turns color off.");

/// Color on for a terminal, even one that says it is `dumb`.
pub const CLICOLOR_FORCE: EnvVar = EnvVar::global(
    "CLICOLOR_FORCE",
    "Any non-empty value turns color on for a terminal, even a `dumb` one; `NO_COLOR` still wins.",
);

/// The terminal type.
pub const TERM: EnvVar = EnvVar::global("TERM", "The terminal type, such as `dumb`.");

/// The width of a terminal, in columns.
pub const COLUMNS: EnvVar = EnvVar::global(
    "COLUMNS",
    "The width in columns that a table must fit, when it is a positive number.",
);

/// The home directory.
pub const HOME: EnvVar = EnvVar::global(
    "HOME",
    "The home directory; the default data directory is under it.",
);

/// The user the environment claims, recorded as the self-reported actor.
pub const USER: EnvVar = EnvVar::global(
    "USER",
    "The user the environment claims, recorded as the self-reported actor; a claim, not an identity.",
);

/// The fallback for the claimed user, on every platform.
pub const USERNAME: EnvVar = EnvVar::global(
    "USERNAME",
    "The claimed user when the one above is unset, empty or not UTF-8, on every platform.",
);

/// Every variable the kit reads, in the order its table lists them.
pub const KIT_VARS: &[EnvVar] = &[
    ROOT,
    FORMAT,
    LOG,
    NO_COLOR,
    CLICOLOR_FORCE,
    TERM,
    COLUMNS,
    HOME,
    USER,
    USERNAME,
];

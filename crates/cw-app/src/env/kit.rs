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

/// The terminal type.
pub const TERM: EnvVar = EnvVar::global("TERM", "The terminal type, such as `dumb`.");

/// The home directory.
pub const HOME: EnvVar = EnvVar::global(
    "HOME",
    "The home directory; the default data directory is under it.",
);

/// The user the environment claims, recorded as the actor.
pub const USER: EnvVar = EnvVar::global(
    "USER",
    "The user the environment claims, recorded as the actor; a claim, not an identity.",
);

/// The fallback for the claimed user, on every platform.
pub const USERNAME: EnvVar = EnvVar::global(
    "USERNAME",
    "The claimed user when the one above is unset, empty or not UTF-8, on every platform.",
);

/// Every variable the kit reads, in the order its table lists them.
pub const KIT_VARS: &[EnvVar] = &[ROOT, FORMAT, LOG, NO_COLOR, TERM, HOME, USER, USERNAME];

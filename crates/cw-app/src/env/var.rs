//! One environment variable, declared once: its name, what it means, its
//! default and the flag it stands in for.

/// How a variable is named.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VarName {
    /// The same name for every program, such as `NO_COLOR`.
    Global(&'static str),
    /// A name under the program's prefix: the suffix `ROOT` is `NOTES_ROOT`
    /// for `notes` (see [`super::AppEnv`] for the prefix rule).
    App(&'static str),
}

/// The declaration of one environment variable a program reads.
///
/// Every field is static text, so a declaration is a `const`; a program
/// declares each variable once and reads it through
/// [`super::AppEnv::get`], so its name is written in one place and the
/// usage table ([`super::table`]) cannot drift from what the code reads.
///
/// ```
/// use cw_app::env::EnvVar;
///
/// const STEP: EnvVar = EnvVar::app("STEP", "How much `add` adds when no amount is given.")
///     .with_default("`1`")
///     .with_flag("--step");
/// assert_eq!(STEP.name_for("tally"), "TALLY_STEP");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EnvVar {
    name: VarName,
    meaning: &'static str,
    default: Option<&'static str>,
    flag: Option<&'static str>,
}

impl EnvVar {
    /// A variable named `name` for every program, meaning `meaning` (one
    /// sentence, Markdown).
    #[must_use]
    pub const fn global(name: &'static str, meaning: &'static str) -> Self {
        Self {
            name: VarName::Global(name),
            meaning,
            default: None,
            flag: None,
        }
    }

    /// A variable named `<APP>_<suffix>`, meaning `meaning` (one sentence,
    /// Markdown).
    #[must_use]
    pub const fn app(suffix: &'static str, meaning: &'static str) -> Self {
        Self {
            name: VarName::App(suffix),
            meaning,
            default: None,
            flag: None,
        }
    }

    /// The same declaration, with what applies when the variable is unset
    /// (Markdown).
    #[must_use]
    pub const fn with_default(mut self, default: &'static str) -> Self {
        self.default = Some(default);
        self
    }

    /// The same declaration, naming the flag that overrides it, such as
    /// `--root`.
    #[must_use]
    pub const fn with_flag(mut self, flag: &'static str) -> Self {
        self.flag = Some(flag);
        self
    }

    /// How the variable is named.
    #[must_use]
    pub const fn name(&self) -> VarName {
        self.name
    }

    /// What the variable means.
    #[must_use]
    pub const fn meaning(&self) -> &'static str {
        self.meaning
    }

    /// What applies when it is unset, if the declaration says.
    #[must_use]
    pub const fn default(&self) -> Option<&'static str> {
        self.default
    }

    /// The flag that overrides it, if any.
    #[must_use]
    pub const fn flag(&self) -> Option<&'static str> {
        self.flag
    }

    /// The variable's full name for the program named `app`: a global name
    /// as declared, an app variable under the app's prefix.
    #[must_use]
    pub fn name_for(&self, app: &str) -> String {
        match self.name {
            VarName::Global(name) => name.to_owned(),
            VarName::App(suffix) => format!("{}_{suffix}", prefix(app)),
        }
    }
}

/// The prefix of `app`'s variables: the name upper-cased, every character
/// that is not an ASCII letter or digit written as `_`.
pub(super) fn prefix(app: &str) -> String {
    app.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

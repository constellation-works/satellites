//! What a colour means (STD-01 R18).

/// The closed set of colour roles a cell or a diagnostic is shown in.
///
/// A program maps its own values onto these in one table of its own; a value
/// it does not map is [`Role::Neutral`]. Each role is one code of the basic
/// 16-colour palette, and a colour never carries meaning alone: with every
/// escape stripped, only emphasis is lost.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Role {
    /// Settled well.
    Ok,
    /// Needs attention.
    Warn,
    /// Failed or broken.
    Error,
    /// Live, and asking for nothing yet.
    Active,
    /// Secondary or de-emphasised.
    Muted,
    /// Plain text.
    #[default]
    Neutral,
}

impl Role {
    /// The SGR parameters for the role, or `None` for plain text.
    fn sgr(self) -> Option<&'static str> {
        match self {
            Self::Ok => Some("32"),
            Self::Warn => Some("33"),
            Self::Error => Some("31"),
            Self::Active => Some("36"),
            Self::Muted => Some("90"),
            Self::Neutral => None,
        }
    }

    /// `text` in this role's colour when `color` is on, else as it is.
    /// Blank text is never wrapped, so padding stays plain.
    #[must_use]
    pub fn paint(self, text: &str, color: bool) -> String {
        match self.sgr() {
            Some(code) if color && !text.trim().is_empty() => {
                format!("\x1b[{code}m{text}\x1b[0m")
            }
            Some(_) | None => text.to_owned(),
        }
    }
}

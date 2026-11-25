//! The usage table: every variable a program reads, as Markdown.

use super::kit::KIT_VARS;
use super::var::EnvVar;

/// The Markdown table of the variables the program named `app` reads: the
/// kit's ([`super::KIT_VARS`]) first, then `own`, each in the order given.
///
/// The output depends on nothing but its arguments, so a program's golden
/// test can pin its usage document to it. Columns: the full name, its
/// meaning, its default and the flag that overrides it; an absent default
/// or flag is `—`. A `|` in the text is escaped and a line break is
/// written as `<br>`, so each declaration is one row.
///
/// ```
/// use cw_app::env::{EnvVar, table};
///
/// const STEP: EnvVar = EnvVar::app("STEP", "How much `add` adds.").with_default("`1`");
/// let text = table("tally", &[STEP]);
/// assert!(text.starts_with("| Variable | Meaning | Default | Flag |\n"));
/// assert!(text.ends_with("| `TALLY_STEP` | How much `add` adds. | `1` | — |\n"));
/// ```
#[must_use]
pub fn table(app: &str, own: &[EnvVar]) -> String {
    let mut out = String::from("| Variable | Meaning | Default | Flag |\n|---|---|---|---|\n");
    for var in KIT_VARS.iter().chain(own) {
        out.push_str(&format!(
            "| `{}` | {} | {} | {} |\n",
            var.name_for(app),
            cell(var.meaning()),
            var.default().map_or_else(|| "—".to_owned(), cell),
            var.flag()
                .map_or_else(|| "—".to_owned(), |flag| format!("`{}`", cell(flag))),
        ));
    }
    out
}

/// `text` safe inside a table cell: a `|` escaped, and every line break
/// written as `<br>`, so a declaration always stays on one row (a blank
/// line would end the table).
fn cell(text: &str) -> String {
    text.replace('|', "\\|")
        .replace("\r\n", "<br>")
        .replace(['\n', '\r'], "<br>")
}

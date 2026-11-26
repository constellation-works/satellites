//! Borderless tables (STD-01 R14, R15) and the piped form of the same rows
//! (STD-01 R9).
//!
//! As a table: one header row, exactly one line per record, two-space
//! gutters, numbers right-aligned, `-` for an absent cell. Widths are
//! measured in terminal display columns, so a wide character counts two.
//! When a width is given (stdout is a terminal), a column marked truncatable
//! shrinks and a cut value ends in a single `…`; with no width nothing is
//! ever cut. Piped: one tab-separated line per record and no header.
//!
//! Every cell is escaped first ([`super::escape`]), so a value cannot break
//! its line or reach the terminal as control.

use super::escape::escape;
use super::style::{Color, Role};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// The narrowest a truncatable column shrinks to, unless its header is
/// wider.
const MIN_TRUNCATED_WIDTH: usize = 8;

/// What an absent cell shows.
const ABSENT: &str = "-";

/// Where a column's cells sit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Align {
    /// Text.
    #[default]
    Left,
    /// Numbers.
    Right,
}

/// A column: its header and how its cells are laid out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Column {
    header: String,
    align: Align,
    truncate: bool,
}

impl Column {
    /// A left-aligned column that is never cut. A numeric column carries its
    /// unit in the header (`DURATION (ms)`), not in every cell.
    #[must_use]
    pub fn new(header: impl Into<String>) -> Self {
        Self {
            header: header.into(),
            align: Align::Left,
            truncate: false,
        }
    }

    /// The same column, right-aligned: for numbers.
    #[must_use]
    pub fn right(self) -> Self {
        Self {
            align: Align::Right,
            ..self
        }
    }

    /// The same column, which may be cut to fit a terminal. Mark only a
    /// column whose full value the program shows elsewhere, in a detail
    /// command or its JSON (STD-01 R15).
    #[must_use]
    pub fn truncatable(self) -> Self {
        Self {
            truncate: true,
            ..self
        }
    }
}

/// One cell: its text, or nothing, and the role it is coloured in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    text: Option<String>,
    role: Role,
}

impl Cell {
    /// A cell holding `text`, with no colour.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            role: Role::Neutral,
        }
    }

    /// An absent value: it shows as `-`, never as blank.
    #[must_use]
    pub fn absent() -> Self {
        Self::default()
    }

    /// The same cell, coloured in `role` where colour is on.
    #[must_use]
    pub fn role(self, role: Role) -> Self {
        Self { role, ..self }
    }

    fn text(&self) -> &str {
        self.text.as_deref().unwrap_or(ABSENT)
    }
}

impl From<&str> for Cell {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

impl From<String> for Cell {
    fn from(text: String) -> Self {
        Self::new(text)
    }
}

impl<T: Into<Cell>> From<Option<T>> for Cell {
    fn from(value: Option<T>) -> Self {
        value.map_or_else(Self::absent, Into::into)
    }
}

/// The human view of a list: columns and one row per record.
///
/// A row shorter than the columns shows `-` in each missing cell; cells past
/// the last column are not shown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Table {
    columns: Vec<Column>,
    rows: Vec<Vec<Cell>>,
}

impl Table {
    /// A table with these columns and no rows.
    #[must_use]
    pub fn new(columns: impl IntoIterator<Item = Column>) -> Self {
        Self {
            columns: columns.into_iter().collect(),
            rows: Vec::new(),
        }
    }

    /// Add one record's row.
    pub fn push(&mut self, cells: impl IntoIterator<Item = impl Into<Cell>>) {
        self.rows.push(cells.into_iter().map(Into::into).collect());
    }

    /// The same table with one more row.
    #[must_use]
    pub fn row(mut self, cells: impl IntoIterator<Item = impl Into<Cell>>) -> Self {
        self.push(cells);
        self
    }

    /// The number of records.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the table has no records.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The table as aligned lines with a header, fitted to `width` when one
    /// is given, each line ending in `\n`. A table with no records is
    /// nothing, not a lone header: stdout stays empty for a query that
    /// matched nothing (STD-01 R16), and the line saying so is the caller's,
    /// on stderr.
    pub(crate) fn render(&self, width: Option<usize>, color: Color) -> String {
        if self.rows.is_empty() {
            return String::new();
        }
        let header: Vec<(String, Role)> = self
            .columns
            .iter()
            .map(|column| (escape(&column.header).into_owned(), Role::Neutral))
            .collect();
        let body: Vec<Vec<(String, Role)>> = self.rows.iter().map(|row| self.cells(row)).collect();
        let natural = self.natural_widths(&header, &body);
        let widths = self.fit(&natural, width);
        let mut out = String::new();
        self.push_line(&mut out, &widths, &header, false);
        for row in &body {
            self.push_line(&mut out, &widths, row, color.is_on());
        }
        out
    }

    /// The table as one tab-separated line per record and no header.
    pub(crate) fn render_plain(&self) -> String {
        let mut out = String::new();
        for row in &self.rows {
            let cells: Vec<String> = self.cells(row).into_iter().map(|(text, _)| text).collect();
            out.push_str(&cells.join("\t"));
            out.push('\n');
        }
        out
    }

    /// A row's escaped text and role, one per column.
    fn cells(&self, row: &[Cell]) -> Vec<(String, Role)> {
        (0..self.columns.len())
            .map(|i| match row.get(i) {
                Some(cell) => (escape(cell.text()).into_owned(), cell.role),
                None => (ABSENT.to_owned(), Role::Neutral),
            })
            .collect()
    }

    fn natural_widths(
        &self,
        header: &[(String, Role)],
        body: &[Vec<(String, Role)>],
    ) -> Vec<usize> {
        header
            .iter()
            .enumerate()
            .map(|(i, (heading, _))| {
                body.iter()
                    .filter_map(|row| row.get(i))
                    .map(|(text, _)| text.width())
                    .fold(heading.width(), usize::max)
            })
            .collect()
    }

    /// Shrink truncatable columns, first to last, until a line fits `width`,
    /// as far as their floors allow.
    fn fit(&self, natural: &[usize], width: Option<usize>) -> Vec<usize> {
        let mut widths = natural.to_vec();
        let Some(limit) = width else {
            return widths;
        };
        let gutters = 2 * self.columns.len().saturating_sub(1);
        let mut excess = (widths.iter().sum::<usize>() + gutters).saturating_sub(limit);
        for (column, width) in self.columns.iter().zip(widths.iter_mut()) {
            if excess == 0 {
                break;
            }
            if column.truncate {
                let floor = MIN_TRUNCATED_WIDTH.max(column.header.width());
                let give = excess.min(width.saturating_sub(floor));
                *width -= give;
                excess -= give;
            }
        }
        widths
    }

    fn push_line(&self, out: &mut String, widths: &[usize], cells: &[(String, Role)], color: bool) {
        let last = self.columns.len().saturating_sub(1);
        for (i, ((column, &width), (text, role))) in
            self.columns.iter().zip(widths).zip(cells).enumerate()
        {
            let text = truncate(text, width);
            let pad = " ".repeat(width.saturating_sub(text.width()));
            let painted = role.paint(&text, Color::of(color));
            match column.align {
                Align::Right => {
                    out.push_str(&pad);
                    out.push_str(&painted);
                }
                Align::Left if i == last => out.push_str(&painted),
                Align::Left => {
                    out.push_str(&painted);
                    out.push_str(&pad);
                }
            }
            if i != last {
                out.push_str("  ");
            }
        }
        out.truncate(out.trim_end_matches(' ').len());
        out.push('\n');
    }
}

/// `text` cut to `width` display columns, ending in one `…` when anything
/// was cut. A kept part that already ends in `…` gets no second one.
fn truncate(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    let keep = width.saturating_sub(1);
    let mut cut = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > keep {
            break;
        }
        cut.push(c);
        used += w;
    }
    if width > 0 && !cut.ends_with('…') {
        cut.push('…');
    }
    cut
}

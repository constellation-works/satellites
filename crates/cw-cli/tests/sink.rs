//! The sink: stdout carries only the payload, and tables follow STD-01.

use cw_app::Env;
use cw_cli::{Cell, Column, Format, Probe, Sink, Table, Terminal};
use serde::Serialize;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(Serialize)]
struct Note {
    id: &'static str,
    title: Option<&'static str>,
}

const NOTES: [Note; 2] = [
    Note {
        id: "n1",
        title: Some("draft"),
    },
    Note {
        id: "n2",
        title: None,
    },
];

fn table(notes: &[Note]) -> Table {
    let mut table = Table::new([Column::new("ID"), Column::new("TITLE")]);
    for note in notes {
        table.push([
            Cell::new(note.id),
            Cell::from(note.title.map(str::to_owned)),
        ]);
    }
    table
}

fn sink(probe: Probe, format: Option<Format>) -> Sink<Vec<u8>, Vec<u8>> {
    let terminal = Terminal::decide(&Env::default(), "notes", probe, format);
    Sink::new(terminal, Vec::new(), Vec::new())
}

/// Emit `table` as the human view in a sink on `probe`, and return stdout.
fn render(table: &Table, probe: Probe) -> Result<String, Box<dyn std::error::Error>> {
    let mut sink = sink(probe, None);
    sink.emit(&(), |()| table.clone())?;
    sink.finish()?;
    Ok(String::from_utf8(sink.into_parts().0)?)
}

#[test]
fn stdout_carries_only_the_payload_in_every_mode() -> TestResult {
    let cases = [
        // stdout on a terminal, stderr redirected, so the stderr is plain.
        (
            Probe {
                stdout: true,
                stderr: false,
                columns: None,
            },
            None,
            "ID  TITLE\nn1  draft\nn2  -\n",
        ),
        (Probe::piped(), None, "n1\tdraft\nn2\t-\n"),
        (
            Probe::piped(),
            Some(Format::Json),
            "[{\"id\":\"n1\",\"title\":\"draft\"},{\"id\":\"n2\",\"title\":null}]\n",
        ),
    ];
    for (probe, format, expected) in cases {
        let mut sink = sink(probe, format);
        sink.notice("2 notes");
        sink.emit(&NOTES[..], table)?;
        sink.warn("the store is old");
        sink.finish()?;
        let (out, err) = sink.into_parts();
        assert_eq!(String::from_utf8(out)?, expected);
        assert_eq!(
            String::from_utf8(err)?,
            "2 notes\nwarning: the store is old\n"
        );
    }
    Ok(())
}

#[test]
fn a_table_is_borderless_with_one_line_per_record() -> TestResult {
    let table = Table::new([
        Column::new("NAME"),
        Column::new("SIZE (B)").right(),
        Column::new("NOTE"),
    ])
    .row([Cell::new("alpha"), Cell::new("7"), Cell::absent()])
    .row([Cell::new("b"), Cell::new("1200"), Cell::new("kept")]);
    assert_eq!(
        render(&table, Probe::terminal(None))?,
        "NAME   SIZE (B)  NOTE\n\
         alpha         7  -\n\
         b          1200  kept\n"
    );
    Ok(())
}

#[test]
fn a_value_is_cut_with_one_ellipsis_only_on_a_terminal() -> TestResult {
    let long = "a title much longer than the terminal is wide";
    let table = Table::new([Column::new("ID"), Column::new("TITLE").truncatable()])
        .row([Cell::new("n1"), Cell::new(long)]);
    let narrow = render(&table, Probe::terminal(Some(20)))?;
    assert_eq!(narrow, "ID  TITLE\nn1  a title much lo…\n");
    assert_eq!(narrow.matches('…').count(), 1);
    // Piped, and on a terminal of unknown width, nothing is cut.
    assert_eq!(render(&table, Probe::piped())?, format!("n1\t{long}\n"));
    assert_eq!(
        render(&table, Probe::terminal(None))?,
        format!("ID  TITLE\nn1  {long}\n")
    );
    Ok(())
}

#[test]
fn widths_are_measured_in_display_columns() -> TestResult {
    let table = Table::new([Column::new("NAME"), Column::new("NEXT")])
        .row([Cell::new("日本語"), Cell::new("x")])
        .row([Cell::new("abc"), Cell::new("y")]);
    // Three wide characters take six columns, so `abc` pads to six.
    assert_eq!(
        render(&table, Probe::terminal(None))?,
        "NAME    NEXT\n日本語  x\nabc     y\n"
    );
    let cut = Table::new([Column::new("ID"), Column::new("TITLE").truncatable()])
        .row([Cell::new("n1"), Cell::new("漢字漢字漢字漢字漢字")]);
    // Twelve columns leave eight for the title: three wide characters and `…`.
    assert_eq!(
        render(&cut, Probe::terminal(Some(12)))?,
        "ID  TITLE\nn1  漢字漢…\n"
    );
    Ok(())
}

#[test]
fn control_characters_in_a_value_are_escaped() -> TestResult {
    let table = Table::new([Column::new("ID"), Column::new("TITLE")])
        .row([Cell::new("n1"), Cell::new("red\u{1b}[31m\tline\nnext\u{7}")]);
    assert_eq!(
        render(&table, Probe::terminal(None))?,
        "ID  TITLE\nn1  red\\u{1b}[31m\\tline\\nnext\\u{7}\n"
    );
    assert_eq!(
        render(&table, Probe::piped())?,
        "n1\tred\\u{1b}[31m\\tline\\nnext\\u{7}\n"
    );
    Ok(())
}

#[test]
fn an_empty_list_writes_no_header_and_json_still_writes_its_array() -> TestResult {
    for (probe, format, expected) in [
        (Probe::terminal(None), None, ""),
        (Probe::piped(), None, ""),
        (Probe::piped(), Some(Format::Json), "[]\n"),
    ] {
        let mut sink = sink(probe, format);
        sink.emit(&NOTES[..0], table)?;
        sink.finish()?;
        assert_eq!(String::from_utf8(sink.into_parts().0)?, expected);
    }
    Ok(())
}

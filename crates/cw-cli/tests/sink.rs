//! The sink: stdout carries only the payload, and tables follow STD-01.

use cw_app::Env;
use cw_cli::{Cell, Column, Format, Probe, Role, Sink, Table, Terminal, TextSpan};
use serde::Serialize;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn text_spans_escape_before_painting_and_respect_colour() -> TestResult {
    for (probe, expected) in [
        (
            Probe::terminal(None),
            "Priority: \x1b[33mhigh\\u{1b}[31m\x1b[0m\n",
        ),
        (Probe::piped(), "Priority: high\\u{1b}[31m\n"),
    ] {
        let mut sink = sink(probe, None);
        sink.emit_text_spans(&"high\x1b[31m", |value| {
            vec![
                TextSpan::new("Priority: "),
                TextSpan::new(*value).role(Role::Warn),
            ]
        })?;
        sink.finish()?;
        assert_eq!(String::from_utf8(sink.into_parts().0)?, expected);
    }
    let mut sink = sink(Probe::piped(), Some(Format::Json));
    sink.emit_text_spans(&"value", |_| panic!("JSON must not build spans"))?;
    sink.finish()?;
    assert_eq!(String::from_utf8(sink.into_parts().0)?, "\"value\"\n");
    Ok(())
}

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
    // A kept part that already ends in `…` gets no second one.
    let ellipsis = Table::new([Column::new("TITLE").truncatable()]).row([Cell::new("123456…AB")]);
    assert_eq!(
        render(&ellipsis, Probe::terminal(Some(8)))?,
        "TITLE\n123456…\n"
    );
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
fn notice_and_warn_escape_every_control_character() -> TestResult {
    let hostile = "path\nforged error: boom\r\ttab\u{1b}[31m";
    let escaped = "path\\nforged error: boom\\r\\ttab\\u{1b}[31m";
    let mut sink = sink(Probe::piped(), None);
    sink.notice(hostile);
    sink.warn(hostile);
    sink.finish()?;
    assert_eq!(
        String::from_utf8(sink.into_parts().1)?,
        format!("{escaped}\nwarning: {escaped}\n")
    );
    Ok(())
}

#[test]
fn multiline_stderr_needs_an_explicit_lines_call() -> TestResult {
    let mut sink = sink(Probe::piped(), None);
    sink.notice_lines(["first\nline", "second\rline"]);
    sink.warn_lines(["first\nline", "second\rline"]);
    sink.finish()?;
    assert_eq!(
        String::from_utf8(sink.into_parts().1)?,
        "first\\nline\nsecond\\rline\nwarning: first\\nline\nwarning: second\\rline\n"
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

#[derive(Serialize)]
struct Owner {
    name: &'static str,
    admin: bool,
}

/// A structured payload with a nested object, an array and an empty array,
/// so pretty output differs from compact at every level.
#[derive(Serialize)]
struct Detail {
    id: &'static str,
    tags: Vec<&'static str>,
    owner: Option<Owner>,
    links: Vec<u32>,
}

fn detail() -> Detail {
    Detail {
        id: "n1",
        tags: vec!["draft", "wip"],
        owner: Some(Owner {
            name: "ana",
            admin: true,
        }),
        links: Vec::new(),
    }
}

const DETAIL_COMPACT: &str = "{\"id\":\"n1\",\"tags\":[\"draft\",\"wip\"],\"owner\":{\"name\":\"ana\",\"admin\":true},\"links\":[]}\n";
const DETAIL_PRETTY: &str = "{\n  \"id\": \"n1\",\n  \"tags\": [\n    \"draft\",\n    \"wip\"\n  ],\n  \"owner\": {\n    \"name\": \"ana\",\n    \"admin\": true\n  },\n  \"links\": []\n}\n";

#[test]
fn json_is_compact_by_default_and_pretty_only_when_opted_in() -> TestResult {
    // The expected bytes are pinned here. The pretty form must also match what
    // `to_writer_pretty` writes plus a line break, the call the Scribe adopter
    // makes today.
    assert_eq!(serde_json::to_string(&detail())? + "\n", DETAIL_COMPACT);
    let mut written = Vec::new();
    serde_json::to_writer_pretty(&mut written, &detail())?;
    written.push(b'\n');
    assert_eq!(String::from_utf8(written)?, DETAIL_PRETTY);
    for probe in [Probe::piped(), Probe::terminal(Some(20))] {
        // Default: compact, on every JSON path.
        let mut compact = sink(probe, Some(Format::Json));
        compact.emit(&detail(), |_| Table::default())?;
        compact.emit_text(&detail(), |_| String::new())?;
        compact.emit_text_spans(&detail(), |_| Vec::new())?;
        compact.finish()?;
        assert_eq!(
            String::from_utf8(compact.into_parts().0)?,
            format!("{DETAIL_COMPACT}{DETAIL_COMPACT}{DETAIL_COMPACT}")
        );

        // Opted in: pretty, on the same three paths, one line break after each.
        let mut pretty = sink(probe, Some(Format::Json)).pretty_json();
        pretty.emit(&detail(), |_| Table::default())?;
        pretty.emit_text(&detail(), |_| String::new())?;
        pretty.emit_text_spans(&detail(), |_| Vec::new())?;
        pretty.finish()?;
        assert_eq!(
            String::from_utf8(pretty.into_parts().0)?,
            format!("{DETAIL_PRETTY}{DETAIL_PRETTY}{DETAIL_PRETTY}")
        );
    }
    Ok(())
}

#[test]
fn pretty_json_leaves_tables_and_plain_lines_unchanged() -> TestResult {
    for (probe, expected) in [
        (Probe::terminal(None), "ID  TITLE\nn1  draft\nn2  -\n"),
        (Probe::piped(), "n1\tdraft\nn2\t-\n"),
    ] {
        let mut sink = sink(probe, None).pretty_json();
        sink.emit(&NOTES[..], table)?;
        sink.finish()?;
        assert_eq!(String::from_utf8(sink.into_parts().0)?, expected);
    }
    Ok(())
}

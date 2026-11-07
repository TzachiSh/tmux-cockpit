//! A code file's rows (#365): every source line, numbered, through the shared
//! lexer in `highlight`, with no markdown read into it. Pure, as `render` is:
//! a row is one source line, or one piece of a line wider than the body, and
//! carries that line's number for `e` and for the selection to come.
use super::render::{Row, hard_joined, untab};
use crate::highlight::{self, Lang, Role, State};

/// The rows of `src` at `w` columns, the gutter's room left over: `lead`
/// columns, the widest line number and a space, which [`gutter`] draws from
/// `line` and the screen puts before the cells. Stored, it would be a heap
/// string a row, the most of a row's cost on a file of short lines. A trailing
/// newline ends the last line and adds none; a CRLF's `\r` goes with every
/// other control character `untab` drops.
pub fn render(src: &str, lang: Option<Lang>, w: usize) -> Vec<Row> {
    let mut lines: Vec<&str> = src.split('\n').collect();
    if src.is_empty() || src.ends_with('\n') {
        lines.pop();
    }
    let gw = lines.len().to_string().len();
    let lead = gw + 1;
    let mut st = State::default();
    let mut out = Vec::with_capacity(lines.len());
    for (i, l) in lines.iter().enumerate() {
        let spans = match lang {
            Some(lg) => highlight::line(lg, &mut st, l),
            None => vec![(Role::Plain, *l)],
        };
        for (mut cells, join) in hard_joined(&untab(&spans), w.saturating_sub(lead)) {
            // a row lives as long as the screen: no room spare for pushes to come
            cells.shrink_to_fit();
            out.push(Row {
                cells,
                line: i + 1,
                lead,
                join,
                body: 0,
            });
        }
    }
    out
}

/// Row `i`'s gutter, `lead` columns: its line number, right aligned, on the
/// line's first row, and blank on the rows a wrap adds. `None` for a row with
/// no gutter (markdown's).
pub fn gutter(rows: &[Row], i: usize) -> Option<String> {
    let r = rows.get(i)?;
    let w = r.lead.checked_sub(1)?;
    let first = i == 0 || rows[i - 1].line != r.line;
    Some(if first { format!("{:>w$} ", r.line) } else { " ".repeat(r.lead) })
}

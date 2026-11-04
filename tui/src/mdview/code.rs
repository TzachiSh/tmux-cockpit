//! A code file's rows (#365): every source line, numbered, through the shared
//! lexer in `highlight`, with no markdown read into it. Pure, as `render` is:
//! a row is one source line, or one piece of a line wider than the body, and
//! carries that line's number for `e` and for the selection to come.
use super::render::{Ink, Kind, Row, hard, untab};
use crate::highlight::{self, Lang, Role, State};

/// The rows of `src` at `w` columns. The gutter is the line number, right
/// aligned to the widest, and a space; a wrapped line's later rows leave it
/// blank. A trailing newline ends the last line and adds none; a CRLF's `\r`
/// goes with every other control character `untab` drops.
pub fn render(src: &str, lang: Option<Lang>, w: usize) -> Vec<Row> {
    let mut lines: Vec<&str> = src.split('\n').collect();
    if src.is_empty() || src.ends_with('\n') {
        lines.pop();
    }
    let gw = lines.len().to_string().len();
    let lead = gw + 1;
    let gutter = Ink::of(Kind::Meta);
    let mut st = State::default();
    let mut out = Vec::with_capacity(lines.len());
    for (i, l) in lines.iter().enumerate() {
        let spans = match lang {
            Some(lg) => highlight::line(lg, &mut st, l),
            None => vec![(Role::Plain, *l)],
        };
        for (k, mut cells) in hard(&untab(&spans), w.saturating_sub(lead)).into_iter().enumerate() {
            let num = if k == 0 { format!("{:>gw$} ", i + 1) } else { " ".repeat(lead) };
            cells.insert(0, (num, gutter));
            out.push(Row { cells, line: i + 1, lead });
        }
    }
    out
}

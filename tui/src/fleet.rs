//! The fleet's two headless forms and the widths a row is drawn at.
//!
//! There is no ratatui list here and there will not be one: `prefix r` stays
//! fzf, which gives fuzzy filter, multi-select and the wheel for free. The
//! width profiles stay because `--rows` consumers size columns from them.
use crate::data::{Pane, State, fmt_dur};
use crate::theme::Theme;

/// The glyph pair: status glyph + review mark, the two leading columns.
pub const MARK: u16 = 2;

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Profile {
    pub name: char,
    pub wher: u16,
    pub title: u16,
    pub doing: u16,
    pub age: u16,
    pub loc: u16,
    pub pm: u16,
}

/// R is the list's inner width. The caps come from the terminal, never from a
/// constant: fixed caps are what made the old row overflow and silently drop ±.
pub fn profile(r: u16) -> Profile {
    if r >= 100 {
        Profile {
            name: 'C',
            wher: 11,
            title: 22,
            doing: 28,
            age: 5,
            loc: 20,
            pm: 10,
        }
    } else if r >= 68 {
        Profile {
            name: 'B',
            wher: 10,
            title: 20,
            doing: 24,
            age: 5,
            loc: 0,
            pm: 10,
        }
    } else {
        Profile {
            name: 'A',
            wher: 0,
            title: 20,
            doing: 20,
            age: 5,
            loc: 0,
            pm: 0,
        }
    }
}

/// Which optional columns a width drops, in drop order. The contract the
/// collapse-order test pins.
pub fn dropped(r: u16) -> Vec<&'static str> {
    let p = profile(r);
    ["loc", "pm", "where"]
        .into_iter()
        .filter(|c| match *c {
            "loc" => p.loc == 0,
            "pm" => p.pm == 0,
            _ => p.wher == 0,
        })
        .collect()
}

/// `--rows`, the headless tab-separated form the bash suite can diff.
pub fn rows_text(ps: &[Pane], th: &Theme) -> String {
    ps.iter()
        .map(|p| {
            format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                th.glyph(p.state).0,
                p.id,
                p.wher,
                p.label(),
                if p.state == State::NeedsInput { &p.msg } else { &p.doing },
                p.since.map(fmt_dur).unwrap_or_default(),
                p.loc,
                p.pm
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn rows_json(ps: &[Pane]) -> String {
    let v: Vec<serde_json::Value> = ps
        .iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id, "where": p.wher, "state": p.state.as_str(), "title": p.label(),
                "doing": p.doing, "said": p.said, "asked": p.asked, "msg": p.msg,
                "since": p.since, "loc": p.loc, "diff": p.pm, "review": p.review,
                "session": p.sess, "path": p.path,
            })
        })
        .collect();
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

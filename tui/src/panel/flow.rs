//! The flow renderer (#935): a subset of Mermaid's `sequenceDiagram` drawn as
//! text that fits a width, either a ladder (actors across, steps down) or a
//! numbered list. Pure: no tmux, no clock, no terminal.
//!
//! What it reads: `participant` and `actor` (with `as`), implicit participants,
//! Mermaid's ten arrows (`->>`, `-->>`, `->`, `-->`, `-x`, `--x`, `-)`, `--)`,
//! `<<->>` and `<<-->>`, each drawn with its own line and head in both shapes),
//! `autonumber`, `Note left of`, `right of` and `over` (the ladder draws a note
//! where its place says, the list under its step), and a
//! `%% status: <✓|●|○|✗|!|done|running|pending|failed|needs> [HH:MM]` line
//! after a step. `loop`, `alt`, `opt`, `else` and `end` are dropped and their
//! steps kept. Anything else is an `Err` naming the line, and [`render`] then
//! shows [`not_drawn`]: the reason and the raw lines.
//!
//! Every glyph it draws comes from the caller's [`Glyphs`] tier, so the ASCII
//! tier draws nothing but ASCII from an ASCII source. No line is ever wider
//! than the width, and nothing here panics: the release profile aborts on a
//! panic.
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::theme::Glyphs;

/// The ladder needs this many columns...
pub const LADDER_MIN_WIDTH: usize = 60;
/// ...and at most this many actors; anything else is a list.
pub const LADDER_MAX_ACTORS: usize = 4;

/// A step's status, from its `%% status:` line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Done,
    Running,
    Pending,
    Failed,
    Needs,
}

impl Status {
    pub fn glyph(self, g: &Glyphs) -> &'static str {
        match self {
            Status::Done => g.done,
            Status::Running => g.running,
            Status::Pending => g.idle,
            Status::Failed => g.step_failed,
            Status::Needs => g.step_needs,
        }
    }

    fn parse(s: &str) -> Option<Status> {
        Some(match s {
            "✓" | "done" => Status::Done,
            "●" | "running" => Status::Running,
            "○" | "pending" => Status::Pending,
            "✗" | "failed" => Status::Failed,
            "!" | "needs" => Status::Needs,
            _ => return None,
        })
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Actor {
    pub id: String,
    pub name: String,
}

/// An arrow's shaft: `->` and the like are solid, `-->` and the like dashed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Line {
    Solid,
    Dashed,
}

impl Line {
    fn glyph(self, g: &Glyphs) -> &'static str {
        match self {
            Line::Solid => g.plain.horizontal_top,
            Line::Dashed => g.dashed,
        }
    }
}

/// An arrow's head: none (`->`), filled (`->>`), open (`-)`), a cross (`-x`),
/// or filled at both ends (`<<->>`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Head {
    None,
    Filled,
    Open,
    Cross,
    Both,
}

/// Where a note sits, by actor index: left of or right of one lifeline, or
/// over one or two (`Note over A` is `Over(a, a)`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Place {
    Left(usize),
    Right(usize),
    Over(usize, usize),
}

/// A note other than NEED FROM YOU: its text and where it sits.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Note {
    pub text: String,
    pub place: Place,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Step {
    pub from: usize,
    pub to: usize,
    pub line: Line,
    pub head: Head,
    pub label: String,
    pub status: Option<Status>,
    pub time: Option<String>,
    /// Notes written after this step, other than NEED FROM YOU.
    pub notes: Vec<Note>,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Flow {
    pub actors: Vec<Actor>,
    pub steps: Vec<Step>,
    /// Notes before the first step.
    pub lead: Vec<Note>,
    /// The NEED FROM YOU notes, prefix dropped, for the closing box.
    pub need: Vec<String>,
    pub autonumber: bool,
}

/// How [`draw`] lays a flow out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Ladder,
    List,
}

impl Flow {
    fn actor(&mut self, id: &str) -> usize {
        if let Some(i) = self.actors.iter().position(|a| a.id == id) {
            return i;
        }
        self.actors.push(Actor {
            id: id.into(),
            name: id.into(),
        });
        self.actors.len() - 1
    }

    pub fn has_status(&self) -> bool {
        self.steps.iter().any(|s| s.status.is_some())
    }

    /// The first step not done, once any step carries a status.
    pub fn current(&self) -> Option<usize> {
        if !self.has_status() {
            return None;
        }
        self.steps.iter().position(|s| s.status != Some(Status::Done))
    }
}

/// Untrusted text as [`parse`] keeps it: [`action::strip_escapes`](super::action::strip_escapes)
/// takes each terminal escape sequence out whole, payload and all (#1530);
/// [`action::sanitize`](super::action::sanitize) then drops bidi controls,
/// U+2028/U+2029 and every control but tab and newline; then a tab is a
/// space. A parsed line holds no newline, so no control is left.
fn tidy(s: &str) -> String {
    super::action::sanitize(&super::action::strip_escapes(s)).replace('\t', " ")
}

/// [`tidy`], then any control left (a newline in a field set by hand) is the
/// tier's replacement mark, so a width counts what shows.
fn clean(s: &str, g: &Glyphs) -> String {
    let mut out = String::new();
    for c in tidy(s).chars() {
        if c.is_control() {
            out.push_str(g.replacement);
        } else {
            out.push(c);
        }
    }
    out
}

fn is_id(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// `HH:MM` on a 24-hour clock, 00:00 to 23:59.
fn is_time(s: &str) -> bool {
    let num = |d: &str| d.len() == 2 && d.bytes().all(|b| b.is_ascii_digit());
    match s.split_once(':') {
        Some((h, m)) if num(h) && num(m) => h < "24" && m < "60",
        _ => false,
    }
}

/// `status: <word> [HH:MM]`, the text after `%%`.
fn status_line(rest: &str) -> Result<(Status, Option<String>), String> {
    let mut words = rest.split_whitespace();
    let word = words.next().ok_or("a status line without a status")?;
    let status = Status::parse(word).ok_or_else(|| format!("unknown status {word}"))?;
    let time = match words.next() {
        Some(t) if is_time(t) => Some(t.to_string()),
        Some(t) => return Err(format!("not a time {t}")),
        None => None,
    };
    match words.next() {
        Some(extra) => Err(format!("more after the status: {extra}")),
        None => Ok((status, time)),
    }
}

/// Mermaid's ten arrows, longest first: a spelling that holds a shorter one
/// (`-->>` holds `->>`, `--x` holds `-x`) is tried before it.
const ARROWS: [(&str, Line, Head); 10] = [
    ("<<-->>", Line::Dashed, Head::Both),
    ("<<->>", Line::Solid, Head::Both),
    ("-->>", Line::Dashed, Head::Filled),
    ("->>", Line::Solid, Head::Filled),
    ("--x", Line::Dashed, Head::Cross),
    ("-x", Line::Solid, Head::Cross),
    ("--)", Line::Dashed, Head::Open),
    ("-)", Line::Solid, Head::Open),
    ("-->", Line::Dashed, Head::None),
    ("->", Line::Solid, Head::None),
];

/// One line, trimmed and not empty, into `f`. `depth` counts open blocks.
fn line(f: &mut Flow, l: &str, depth: &mut usize) -> Result<(), String> {
    if let Some(comment) = l.strip_prefix("%%") {
        let comment = comment.trim_start();
        // The key in any case: "%% Status: done" is a status, never a comment.
        let key = comment.get(..7).filter(|k| k.eq_ignore_ascii_case("status:"));
        if let Some(rest) = key.map(|k| &comment[k.len()..]) {
            let (status, time) = status_line(rest)?;
            let step = f.steps.last_mut().ok_or("a status before any step")?;
            step.status = Some(status);
            step.time = time;
        }
        return Ok(());
    }
    let (word, rest) = l.split_once(char::is_whitespace).map_or((l, ""), |(w, r)| (w, r.trim()));
    match word {
        "autonumber" if rest.is_empty() => f.autonumber = true,
        "participant" | "actor" => {
            let (id, name) = rest.split_once(" as ").map_or((rest, rest), |(i, n)| (i.trim(), n.trim()));
            if !is_id(id) || name.is_empty() {
                return Err("a participant needs an id".into());
            }
            let i = f.actor(id);
            f.actors[i].name = tidy(name);
        }
        "Note" | "note" => note(f, rest)?,
        "loop" | "alt" | "opt" => *depth += 1,
        "else" | "end" if *depth == 0 => return Err(format!("{word} outside a block")),
        "else" => {}
        "end" => *depth -= 1,
        _ => step(f, l)?,
    }
    Ok(())
}

fn note(f: &mut Flow, rest: &str) -> Result<(), String> {
    let (place, text) = rest.split_once(':').ok_or("a note without ':'")?;
    let place = place.trim();
    // Each place from the first and the last actor it names: over takes both,
    // left of and right of the first.
    type Make = fn(usize, usize) -> Place;
    let places: [(&str, Make); 3] = [
        ("left of ", |a, _| Place::Left(a)),
        ("right of ", |a, _| Place::Right(a)),
        ("over ", Place::Over),
    ];
    let (ids, make) = places
        .iter()
        .find_map(|&(p, make)| place.strip_prefix(p).map(|ids| (ids, make)))
        .ok_or("a note without left of, right of or over")?;
    let mut ends: Option<(usize, usize)> = None;
    for id in ids.split(',').map(str::trim) {
        if !is_id(id) {
            return Err("a note over no participant".into());
        }
        let i = f.actor(id);
        ends = Some(ends.map_or((i, i), |(first, _)| (first, i)));
    }
    let (first, last) = ends.ok_or("a note over no participant")?;
    let place = make(first, last);
    let text = tidy(text.trim());
    // The prefix ends the word: "NEED FROM YOUR call" is an ordinary note.
    let need = text
        .strip_prefix("NEED FROM YOU")
        .filter(|r| r.is_empty() || r.starts_with(':') || r.starts_with(char::is_whitespace));
    if let Some(need) = need {
        f.need.push(need.trim_start_matches(':').trim().to_string());
    } else if let Some(step) = f.steps.last_mut() {
        step.notes.push(Note { text, place });
    } else {
        f.lead.push(Note { text, place });
    }
    Ok(())
}

fn step(f: &mut Flow, l: &str) -> Result<(), String> {
    let (arrow, label) = l.split_once(':').ok_or("not a step")?;
    let (from, to, line, head) = ARROWS
        .iter()
        .find_map(|&(a, line, head)| arrow.split_once(a).map(|(x, y)| (x.trim(), y.trim(), line, head)))
        .ok_or("no arrow")?;
    if !is_id(from) || !is_id(to) {
        return Err("a step needs two participant ids".into());
    }
    let (from, to) = (f.actor(from), f.actor(to));
    f.steps.push(Step {
        from,
        to,
        line,
        head,
        label: tidy(label.trim()),
        status: None,
        time: None,
        notes: vec![],
    });
    Ok(())
}

/// Reads the subset in the module doc; an `Err` says why and on which line.
pub fn parse(src: &str) -> Result<Flow, String> {
    let mut lines = src
        .lines()
        .enumerate()
        .map(|(n, l)| (n + 1, l.trim()))
        .filter(|(_, l)| !l.is_empty());
    match lines.find(|(_, l)| !l.starts_with("%%")) {
        Some((_, "sequenceDiagram")) => {}
        _ => return Err("no sequenceDiagram header".into()),
    }
    let mut f = Flow::default();
    let mut depth = 0;
    for (n, l) in lines {
        line(&mut f, l, &mut depth).map_err(|why| format!("line {n}: {why}: {l}"))?;
    }
    if depth > 0 {
        return Err("a loop, alt or opt is never closed".into());
    }
    if f.steps.is_empty() {
        return Err("no steps".into());
    }
    Ok(f)
}

/// The ladder at [`LADDER_MIN_WIDTH`] or more with at most
/// [`LADDER_MAX_ACTORS`], otherwise the list.
pub fn shape(f: &Flow, w: usize) -> Shape {
    if w >= LADDER_MIN_WIDTH && f.actors.len() <= LADDER_MAX_ACTORS {
        Shape::Ladder
    } else {
        Shape::List
    }
}

/// Cut `s` to at most `w` columns, ending in the tier's ellipsis when cut.
/// Below the ellipsis's width it keeps the first `w` columns and no ellipsis.
/// At exactly that width it gives the ellipsis alone, where [`Glyphs::trunc`]
/// keeps the first columns. Cuts between grapheme clusters, so an emoji
/// sequence is kept whole or dropped whole.
fn fit(s: &str, w: usize, g: &Glyphs) -> String {
    if s.width() <= w {
        return s.to_string();
    }
    let e = g.ellipsis.width();
    let (room, end) = if w < e { (w, "") } else { (w - e, g.ellipsis) };
    let mut kept: Vec<&str> = vec![];
    let mut used = 0;
    for c in s.graphemes(true) {
        if used + c.width() > room {
            break;
        }
        kept.push(c);
        used += c.width();
    }
    // Clusters can measure narrower apart than joined.
    while kept.concat().width() > room {
        kept.pop();
    }
    kept.concat() + end
}

/// Words into lines of at most `w` columns; a word wider than that is cut.
fn wrap(s: &str, w: usize, g: &Glyphs) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    for word in s.split_whitespace() {
        if !cur.is_empty() && cur.width() + 1 + word.width() > w {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(&fit(word, w, g));
    }
    if !cur.is_empty() || out.is_empty() {
        out.push(cur);
    }
    out
}

/// A note in rows of at most `w` columns: the tier's note mark and a space
/// before the first row, as many spaces before each later one, so a note that
/// wraps still reads as one note (#1531).
fn note_rows(text: &str, w: usize, g: &Glyphs) -> Vec<String> {
    let mark = format!("{} ", g.note);
    let pad = " ".repeat(mark.width());
    wrap(text, w.saturating_sub(mark.width()), g)
        .into_iter()
        .enumerate()
        .map(|(i, row)| format!("{}{row}", if i == 0 { &mark } else { &pad }))
        .collect()
}

/// The fewest columns a ladder note gets. A layout floor, not a fallback
/// value: a region its place leaves narrower (right of the last actor, left of
/// the first, over one lifeline between near neighbours) grows to this and may
/// then cover the lifeline it sits by, so the note still has room to read
/// (#1533).
const NOTE_MIN: usize = 16;

/// Actor `i`'s lifeline column in `cols`. A step or a note built by hand may
/// name an actor past the end: it draws at the last one.
fn lifeline(cols: &[usize], i: usize) -> usize {
    cols.get(i).or(cols.last()).copied().unwrap_or_default()
}

/// A ladder note's columns `[start, end)`, from the lifeline columns `cols`
/// and the columns `lo..hi` a note may use (#1533). Right of a lifeline it
/// starts two columns after it, left of one it ends two columns before it,
/// over two it spans them, and over one it reaches halfway to the lifeline on
/// each side, or to `lo` or `hi` where there is none. A region under
/// [`NOTE_MIN`] grows to it about its centre, then slides back inside `lo..hi`.
fn note_region(place: &Place, cols: &[usize], lo: usize, hi: usize) -> (usize, usize) {
    let at = |i: usize| lifeline(cols, i);
    let (start, end) = match *place {
        Place::Right(a) => (at(a) + 2, hi),
        Place::Left(a) => (lo, at(a).saturating_sub(1)),
        Place::Over(a, b) if at(a) == at(b) => {
            let c = at(a);
            let before = cols.iter().copied().filter(|&l| l < c).max();
            let after = cols.iter().copied().filter(|&l| l > c).min();
            (before.map_or(lo, |l| l.midpoint(c)), after.map_or(hi, |r| c.midpoint(r)))
        }
        Place::Over(a, b) => (at(a).min(at(b)), at(a).max(at(b)) + 1),
    };
    let width = end.saturating_sub(start).max(NOTE_MIN).min(hi.saturating_sub(lo));
    let start = ((start + end).saturating_sub(width) / 2).min(hi.saturating_sub(width)).max(lo);
    (start, start + width)
}

/// A row of cells written at columns, one grapheme cluster a cell. A wide
/// cluster takes two cells, the second empty; overwriting either half blanks
/// the other.
struct Row(Vec<String>);

impl Row {
    fn new(w: usize) -> Row {
        Row(vec![" ".into(); w])
    }

    /// Blank the other half of the wide cluster at `col`, if any.
    fn unpair(&mut self, col: usize) {
        match self.0.get(col) {
            Some(c) if c.is_empty() && col > 0 => self.0[col - 1] = " ".into(),
            Some(c) if c.width() == 2 => {
                if let Some(next) = self.0.get_mut(col + 1) {
                    *next = " ".into();
                }
            }
            _ => {}
        }
    }

    fn put(&mut self, col: usize, g: &str) {
        if col < self.0.len() {
            self.unpair(col);
            self.0[col] = g.into();
        }
    }

    fn text(&mut self, col: usize, s: &str) {
        let mut at = col;
        for g in s.graphemes(true) {
            let gw = g.width();
            if gw == 0 || gw > 2 {
                continue;
            }
            if at + gw > self.0.len() {
                break;
            }
            if gw == 2 {
                // The right half first: unpairing it may blank `at`, not yet written.
                self.unpair(at + 1);
                self.put(at, g);
                self.0[at + 1] = String::new();
            } else {
                self.put(at, g);
            }
            at += gw;
        }
    }

    fn done(self) -> String {
        self.0.concat().trim_end().to_string()
    }
}

fn label(f: &Flow, i: usize) -> String {
    if f.autonumber {
        format!("{} {}", i + 1, f.steps[i].label)
    } else {
        f.steps[i].label.clone()
    }
}

/// The glyphs at the left and the right end of a shaft as drawn, `""` where
/// there is no head. A step drawn right to left has its head at the left end,
/// pointing left; a two-way arrow has one at each end either way. Both shapes
/// draw their ends from here (#1532).
fn arrow_ends(head: Head, rightward: bool, g: &Glyphs) -> (&'static str, &'static str) {
    // The head pointing right, then pointing left.
    let (right, left) = match head {
        Head::None => ("", ""),
        Head::Filled => (g.head_right, g.head_left),
        Head::Open => (g.open_right, g.open_left),
        Head::Cross => (g.head_cross, g.head_cross),
        Head::Both => return (g.head_left, g.head_right),
    };
    if rightward { ("", right) } else { (left, "") }
}

/// The closing box of NEED FROM YOU notes, the width wide.
fn need_box(f: &Flow, w: usize, g: &Glyphs) -> Vec<String> {
    if f.need.is_empty() {
        return vec![];
    }
    if w < 6 {
        return f.need.iter().map(|n| fit(&format!("NEED FROM YOU: {n}"), w, g)).collect();
    }
    let b = g.plain;
    let inner = w - 4;
    let title = fit(" NEED FROM YOU ", w - 2, g);
    let top = b.horizontal_top.repeat(w - 2 - title.width());
    let mut out = vec![format!("{}{title}{top}{}", b.top_left, b.top_right)];
    for line in f.need.iter().flat_map(|n| wrap(n, inner, g)) {
        let pad = " ".repeat(inner.saturating_sub(line.width()));
        out.push(format!("{} {line}{pad} {}", b.vertical_left, b.vertical_right));
    }
    out.push(format!("{}{}{}", b.bottom_left, b.horizontal_bottom.repeat(w - 2), b.bottom_right));
    out
}

/// Actors across, two rows a step: the label, then the arrow under it.
fn ladder(f: &Flow, w: usize, g: &Glyphs) -> Vec<String> {
    let gutter = if f.has_status() { 3 } else { 1 };
    let right = if f.steps.iter().any(|s| s.time.is_some()) { 6 } else { 0 };
    let slot = (w.saturating_sub(gutter + right) / f.actors.len().max(1)).max(2);
    // One lifeline column an actor; a flow built by hand with no actor still has the first.
    let cols: Vec<usize> = (0..f.actors.len().max(1)).map(|i| gutter + slot * i + slot / 2).collect();
    let x = |i: usize| lifeline(&cols, i);
    let lifelines = || {
        let mut r = Row::new(w);
        for i in 0..f.actors.len() {
            r.put(x(i), g.plain.vertical_left);
        }
        r
    };
    // A note sits where its place says, between the gutter and the time column.
    let notes = |out: &mut Vec<String>, notes: &[Note]| {
        for n in notes {
            let (start, end) = note_region(&n.place, &cols, gutter, w.saturating_sub(right));
            for row in note_rows(&n.text, end - start, g) {
                // Each row starts its region right of a lifeline, ends it left of one, and is centred over them.
                let spare = (end - start).saturating_sub(row.width());
                let pad = match n.place {
                    Place::Right(_) => 0,
                    Place::Left(_) => spare,
                    Place::Over(..) => spare / 2,
                };
                let mut r = lifelines();
                r.text(start + pad, &row);
                out.push(r.done());
            }
        }
    };
    let mut head = Row::new(w);
    for (i, a) in f.actors.iter().enumerate() {
        let name = fit(&a.name, slot - 1, g);
        head.text(x(i).saturating_sub(name.width() / 2), &name);
    }
    let mut out = vec![head.done()];
    notes(&mut out, &f.lead);
    let cur = f.current();
    for (i, s) in f.steps.iter().enumerate() {
        let (a, b) = (x(s.from).min(x(s.to)), x(s.from).max(x(s.to)));
        let mut lab = lifelines();
        if cur == Some(i) {
            lab.put(0, g.head_right);
        }
        if let Some(st) = s.status {
            lab.put(1, st.glyph(g));
        }
        lab.text(a + 2, &fit(&label(f, i), w.saturating_sub(right + a + 2), g));
        if let Some(t) = &s.time {
            lab.text(w - right + 1, &fit(t, right - 1, g));
        }
        out.push(lab.done());
        let mut arrow = lifelines();
        let shaft = s.line.glyph(g);
        if a == b {
            let back = match s.head {
                Head::Filled | Head::Both => g.self_back,
                Head::Open => g.open_left,
                Head::Cross => g.head_cross,
                Head::None => "",
            };
            arrow.text(a + 1, &format!("{shaft}{}{back}", g.self_turn));
        } else {
            for c in a + 1..b {
                arrow.put(c, shaft);
            }
            let (left_end, right_end) = arrow_ends(s.head, s.to > s.from, g);
            if !left_end.is_empty() {
                arrow.put(a + 1, left_end);
            }
            if !right_end.is_empty() {
                arrow.put(b - 1, right_end);
            }
        }
        out.push(arrow.done());
        notes(&mut out, &s.notes);
    }
    out.extend(need_box(f, w, g));
    out
}

/// One numbered line a step: status, number, label, who to whom, time.
fn list(f: &Flow, w: usize, g: &Glyphs) -> Vec<String> {
    let cur = f.current();
    let status = f.has_status();
    let here = format!("{} here", g.head_left);
    let digits = if f.autonumber { f.steps.len().to_string().len().max(2) } else { 0 };
    let longest = f.steps.iter().map(|s| s.label.width()).max().unwrap_or(0);
    let name = |i: usize| f.actors.get(i).map_or("?", |a| a.name.as_str());
    // Who to whom, read left to right: the arrow's left end, its shaft, then
    // its right end, or the shaft again where there is no head.
    let who = |s: &Step| {
        let (left, right) = arrow_ends(s.head, true, g);
        let shaft = s.line.glyph(g);
        let right = if right.is_empty() { shaft } else { right };
        format!("{}{left}{shaft}{right}{}", name(s.from), name(s.to))
    };
    let who_w = f.steps.iter().map(|s| who(s).width()).max().unwrap_or(0);
    let time_w = f.steps.iter().filter_map(|s| s.time.as_deref()).map(str::width).max().unwrap_or(0);
    let lead_w = usize::from(status) * 2 + if f.autonumber { digits + 1 } else { 0 };
    // One label column for every line: aligned when it holds the longest label
    // or 6 columns of it, whichever is less, and the rest of an aligned line
    // fits too (with every label empty the column holds them at any width);
    // compact otherwise. The time column and the gap before it take room only
    // when a step has a time.
    let time_col = if time_w > 0 { 2 + time_w } else { 0 };
    let rest_w = lead_w + 2 + who_w + time_col;
    let room = w.saturating_sub(rest_w).min(longest);
    let aligned = room >= longest.min(6) && rest_w <= w;
    let mut out = vec![];
    for (i, s) in f.steps.iter().enumerate() {
        let mut lead = String::new();
        if status {
            lead.push_str(s.status.map_or(" ", |st| st.glyph(g)));
            lead.push(' ');
        }
        if f.autonumber {
            lead.push_str(&format!("{:>digits$} ", i + 1));
        }
        let who = who(s);
        let time = s.time.as_deref().unwrap_or("");
        let tail = format!("  {who}{}  {time}", " ".repeat(who_w - who.width()));
        let line = if aligned {
            let label = fit(&s.label, room, g);
            format!("{lead}{label}{}{tail}", " ".repeat(room - label.width()))
        } else {
            fit(format!("{lead}{} {who} {time}", s.label).trim_end(), w, g)
        };
        let line = line.trim_end();
        if cur == Some(i) && line.width() + 2 + here.width() <= w {
            out.push(format!("{line}  {here}"));
        } else {
            out.push(line.to_string());
            if cur == Some(i) {
                out.push(fit(&format!("{}{here}", " ".repeat(lead.width())), w, g));
            }
        }
        let indent = " ".repeat(lead.width());
        for line in s.notes.iter().flat_map(|n| note_rows(&n.text, w.saturating_sub(lead.width()), g)) {
            out.push(fit(&format!("{indent}{line}"), w, g));
        }
    }
    let mut head: Vec<String> = f
        .lead
        .iter()
        .flat_map(|n| note_rows(&n.text, w, g))
        .map(|l| fit(&l, w, g))
        .collect();
    head.extend(out);
    head.extend(need_box(f, w, g));
    head
}

/// `f` at width `w`, in the [`shape`] the width and actor count pick, with
/// the glyphs of tier `g`.
pub fn draw(f: &Flow, w: usize, g: &Glyphs) -> Vec<String> {
    let lines = match shape(f, w) {
        Shape::Ladder => ladder(f, w, g),
        Shape::List => list(f, w, g),
    };
    // The layouts aim to fit; this makes it a guarantee. A Flow built by hand
    // never went through parse's tidy, so each line is cleaned here (#955).
    lines.into_iter().map(|l| fit(&clean(&l, g), w, g)).collect()
}

/// What the caller shows for source that does not parse: the reason, then the
/// raw lines, each cut to the width.
pub fn not_drawn(src: &str, why: &str, w: usize, g: &Glyphs) -> Vec<String> {
    std::iter::once(fit(&clean(&format!("(not drawn: {why})"), g), w, g))
        .chain(src.lines().map(|l| fit(&clean(l, g), w, g)))
        .collect()
}

/// [`parse`] then [`draw`], or [`not_drawn`] when it does not parse.
pub fn render(src: &str, w: usize, g: &Glyphs) -> Vec<String> {
    match parse(src) {
        Ok(f) => draw(&f, w, g),
        Err(why) => not_drawn(src, &why, w, g),
    }
}

#[cfg(test)]
mod tests {
    use super::{Actor, Flow, Glyphs, Head, Line, Note, Place, Row, Shape, Status, Step, draw, fit, ladder, list, parse, shape};

    const UNI: Glyphs = Glyphs::unicode();

    /// A character no drawn line may hold: a control, a bidi control, U+2028/U+2029.
    fn unsafe_char(c: char) -> bool {
        let bidi = matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
        c.is_control() || bidi || matches!(c, '\u{2028}' | '\u{2029}')
    }

    /// Every text field set directly, each holding `bad` (#955).
    fn hand_built(bad: &str) -> Flow {
        let actor = |id: &str| Actor {
            id: id.into(),
            name: format!("{id}{bad}name"),
        };
        let step = Step {
            from: 0,
            to: 1,
            line: Line::Solid,
            head: Head::Filled,
            label: format!("deploy{bad}it"),
            status: Some(Status::Running),
            time: Some(format!("1{bad}2")),
            notes: vec![Note {
                text: format!("note{bad}here"),
                place: Place::Right(0),
            }],
        };
        Flow {
            actors: vec![actor("A"), actor("B")],
            steps: vec![step],
            lead: vec![Note {
                text: format!("lead{bad}line"),
                place: Place::Over(0, 1),
            }],
            need: vec![format!("need{bad}this")],
            autonumber: true,
        }
    }

    #[test]
    fn a_hand_built_flow_draws_no_control_or_bidi_character() {
        let rows = [("RLO and BEL", "\u{202E}\u{7}"), ("LRI, LS, newline", "\u{2066}\u{2028}\n")];
        for (row, bad) in rows {
            let f = hand_built(bad);
            for (w, want) in [(100, Shape::Ladder), (40, Shape::List)] {
                assert_eq!(shape(&f, w), want, "{row} w={w}");
                let out = draw(&f, w, &UNI);
                let hit = out.iter().find(|l| l.chars().any(unsafe_char));
                assert_eq!(hit, None, "{row} w={w}");
            }
        }
    }

    /// Cleaning at draw time changes nothing in a parsed flow: its text is clean already.
    #[test]
    fn a_parsed_flow_draws_as_its_layout_fitted() {
        let src = "sequenceDiagram\nautonumber\nparticipant A as Ann ↳\nNote over A: lead\n\
                   A->>B: go ✓ │ …\n%% status: running 09:30\nNote over B: NEED FROM YOU: ok\n";
        let f = parse(src).expect("sample parses");
        type Layout = fn(&Flow, usize, &Glyphs) -> Vec<String>;
        for (w, layout) in [(100, ladder as Layout), (40, list as Layout)] {
            let plain: Vec<String> = layout(&f, w, &UNI).iter().map(|l| fit(l, w, &UNI)).collect();
            assert_eq!(draw(&f, w, &UNI), plain, "w={w}");
        }
    }

    /// No layout writes over a wide cell today; this pins the rule for the next one.
    #[test]
    fn writing_over_either_half_of_a_wide_cell_blanks_the_other() {
        for (row, col, want) in [("over the left half", 2, "  │ x"), ("over the right half", 3, "   │x")] {
            let mut r = Row::new(5);
            r.text(2, "名x");
            r.put(col, "│");
            assert_eq!(r.done(), want.trim_end(), "{row}");
        }
    }
}

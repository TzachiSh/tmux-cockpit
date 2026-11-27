//! The flow renderer (#935): a subset of Mermaid's `sequenceDiagram` drawn as
//! text that fits a width, either a ladder (actors across, steps down) or a
//! numbered list. Pure: no tmux, no clock, no terminal.
//!
//! What it reads: `participant` and `actor` (with `as`), implicit participants,
//! Mermaid's ten arrows (`->>`, `-->>`, `->`, `-->`, `-x`, `--x`, `-)`, `--)`,
//! `<<->>` and `<<-->>`, each drawn with its own line and head in both shapes),
//! `autonumber` with or without a start and an increment, `Note left of`,
//! `right of` and `over` (the ladder draws a note where its place says, the
//! list under its step), and a
//! `%% status: <✓|●|○|✗|!|done|running|pending|failed|needs> [HH:MM]` line
//! after a step. `loop`, `alt`, `opt`, `par`, `critical` and `break` draw as
//! frames around their steps, an `alt`'s `else`, a `par`'s `and` and a
//! `critical`'s `option` as dividers, at most [`MAX_FRAME_DEPTH`] deep.
//! `activate`, `deactivate`, a `+` or `-` before a target, a central
//! connection's `()`, `rect`, `box`, `create` and `destroy` keep the steps and
//! participants they hold and drop the rest, and a ledger line after the steps
//! names what the drawing leaves out (#1535, #1536, #1537). A `;` ends a
//! statement as a new line does, and `#59;` in text reads as `;`. Anything
//! else is an `Err` naming the line, and [`render`] then shows [`not_drawn`]:
//! the reason and the raw lines.
//!
//! Every glyph it draws comes from the caller's [`Glyphs`] tier, so the ASCII
//! tier draws nothing but ASCII from an ASCII source. No line is ever wider
//! than the width, and nothing here panics: the release profile aborts on a
//! panic.
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::theme::Glyphs;

/// The ladder needs this many columns, whatever its slot...
pub const LADDER_MIN_WIDTH: usize = 60;
/// ...and this many columns an actor for its normal spacing...
pub const LADDER_SLOT: usize = 12;
/// ...or this many for its compact one, drawn by the same rules; a narrower
/// slot is a list (#1538).
pub const COMPACT_SLOT: usize = 10;

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

/// A block that `end` closes: one drawn as a frame around its steps (#1536,
/// #1537), or a `rect` or a `box`, which keeps its steps or participants and
/// draws no frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Block {
    Loop,
    Alt,
    Opt,
    Par,
    Critical,
    Break,
    Rect,
    Group,
}

/// Every block, as [`line()`] finds the one a keyword opens or a word divides.
const BLOCKS: &[Block] = &[
    Block::Loop,
    Block::Alt,
    Block::Opt,
    Block::Par,
    Block::Critical,
    Block::Break,
    Block::Rect,
    Block::Group,
];

impl Block {
    /// The keyword that opens the block, as its frame's top rule shows it.
    fn word(self) -> &'static str {
        match self {
            Block::Loop => "loop",
            Block::Alt => "alt",
            Block::Opt => "opt",
            Block::Par => "par",
            Block::Critical => "critical",
            Block::Break => "break",
            Block::Rect => "rect",
            Block::Group => "box",
        }
    }

    /// The word that divides the block into branches, as its divider shows
    /// it: `else` in an `alt`, `and` in a `par`, `option` in a `critical`.
    fn divider(self) -> Option<&'static str> {
        match self {
            Block::Alt => Some("else"),
            Block::Par => Some("and"),
            Block::Critical => Some("option"),
            _ => None,
        }
    }

    /// The ledger's name for a block kept with no frame (#1537).
    fn unframed(self) -> Option<&'static str> {
        match self {
            Block::Rect => Some(RECTS),
            Block::Group => Some(BOXES),
            _ => None,
        }
    }
}

/// A place between drawn rows: after this many steps and this many notes of
/// the last one, or of the notes before the first step when there is none.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Spot {
    pub steps: usize,
    pub notes: usize,
}

/// A block's frame: its label, how many frames it sits inside, where it opens,
/// each divider (`else`, `and` or `option`) with its label, and where it
/// closes.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Frame {
    pub kind: Block,
    pub label: String,
    pub depth: usize,
    pub open: Spot,
    pub dividers: Vec<(Spot, String)>,
    pub close: Spot,
}

/// At most this many frames nest. A block deeper still draws its steps with
/// no frame, and the ledger names the loss (#1536).
pub const MAX_FRAME_DEPTH: usize = 3;

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Flow {
    pub actors: Vec<Actor>,
    pub steps: Vec<Step>,
    /// Notes before the first step.
    pub lead: Vec<Note>,
    /// The NEED FROM YOU notes, prefix dropped, for the closing box.
    pub need: Vec<String>,
    /// What the drawing leaves out, one short name a kind in the order first
    /// met, for the ledger line (#1535).
    pub dropped: Vec<String>,
    /// The frames, in the order their blocks open (#1536).
    pub frames: Vec<Frame>,
    /// `autonumber`'s start and increment, none without it (#1537).
    pub autonumber: Option<(usize, usize)>,
}

/// How [`draw`] lays a flow out: the ladder, the compact ladder (a narrower
/// slot, drawn by the same rules), or the list (#1538).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Ladder,
    Compact,
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

    /// Where the source read so far ends: after its steps and the last one's notes.
    fn here(&self) -> Spot {
        let (steps, notes) = (self.steps.len(), self.steps.last().map_or(self.lead.len(), |s| s.notes.len()));
        Spot { steps, notes }
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

/// A statement's first word and the rest, trimmed.
fn first_word(l: &str) -> (&str, &str) {
    l.split_once(char::is_whitespace).map_or((l, ""), |(w, r)| (w, r.trim()))
}

/// One statement, trimmed and not empty, into `f`: kept, drawn as a frame, or
/// dropped with its name in the ledger and no actor added (#1537). `open`
/// holds each open block, outermost first, with its frame's index in
/// `f.frames`, or none for a `rect`, a `box` or a block past
/// [`MAX_FRAME_DEPTH`].
fn line(f: &mut Flow, l: &str, open: &mut Vec<(Block, Option<usize>)>) -> Result<(), String> {
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
    let (word, rest) = first_word(l);
    match word {
        "autonumber" => f.autonumber = Some(autonumber(rest)?),
        "participant" | "actor" => participant(f, rest)?,
        // The participant is kept, its late start is not.
        "create" => match first_word(rest) {
            ("participant" | "actor", p) => {
                record_drop(f, LIFECYCLE);
                participant(f, p)?;
            }
            _ => return Err("create without a participant".into()),
        },
        "destroy" => record_drop(f, LIFECYCLE),
        "activate" | "deactivate" => record_drop(f, ACTIVATIONS),
        "Note" | "note" => note(f, rest)?,
        "end" => {
            let (_, frame) = open.pop().ok_or("end outside a block")?;
            let at = f.here();
            if let Some(fr) = frame.and_then(|i| f.frames.get_mut(i)) {
                fr.close = at;
            }
        }
        _ => match BLOCKS.iter().find(|b| b.word() == word) {
            Some(&kind) => {
                // How deep its frame sits: a block with no frame around it does not count.
                let depth = open.iter().filter(|(_, fr)| fr.is_some()).count();
                let frame = if let Some(name) = kind.unframed() {
                    record_drop(f, name);
                    None
                } else if depth >= MAX_FRAME_DEPTH {
                    // Its steps are drawn and its frame is not, so the ledger names it.
                    record_drop(f, &format!("frames past depth {MAX_FRAME_DEPTH}"));
                    None
                } else {
                    let at = f.here();
                    f.frames.push(Frame {
                        kind,
                        label: tidy(rest),
                        depth,
                        open: at,
                        dividers: vec![],
                        close: at,
                    });
                    Some(f.frames.len() - 1)
                };
                open.push((kind, frame));
            }
            // A divider word divides the innermost open block, and only one of the kind it divides.
            None if BLOCKS.iter().any(|b| b.divider() == Some(word)) => match open.last() {
                Some(&(kind, frame)) if kind.divider() == Some(word) => {
                    let at = f.here();
                    if let Some(fr) = frame.and_then(|i| f.frames.get_mut(i)) {
                        fr.dividers.push((at, tidy(rest)));
                    }
                }
                _ => return Err(format!("{word} outside the block it divides")),
            },
            None => step(f, l)?,
        },
    }
    Ok(())
}

/// A participant line's rest, `<id>` or `<id> as <name>`, into `f`.
fn participant(f: &mut Flow, rest: &str) -> Result<(), String> {
    let (id, name) = rest.split_once(" as ").map_or((rest, rest), |(i, n)| (i.trim(), n.trim()));
    if !is_id(id) || name.is_empty() {
        return Err("a participant needs an id".into());
    }
    let i = f.actor(id);
    f.actors[i].name = tidy(name);
    Ok(())
}

/// `autonumber`'s rest: nothing, numbering from 1 by 1, or a start and an
/// increment, each a whole number (Mermaid 11.15.0, #1537).
fn autonumber(rest: &str) -> Result<(usize, usize), String> {
    let bad = || format!("autonumber takes a start and an increment, not {rest}");
    let words: Vec<&str> = rest.split_whitespace().collect();
    match words.as_slice() {
        [] => Ok((1, 1)),
        [start, by] => Ok((start.parse().map_err(|_| bad())?, by.parse().map_err(|_| bad())?)),
        _ => Err(bad()),
    }
}

// The ledger's names for what a kept or dropped statement leaves out (#1537).
const ACTIVATIONS: &str = "activations";
const CENTRAL: &str = "central connections";
const RECTS: &str = "rect highlights";
const BOXES: &str = "participant boxes";
const LIFECYCLE: &str = "create/destroy";

/// Name `what` in the ledger once, in the order first met (#1535).
fn record_drop(f: &mut Flow, what: &str) {
    if !f.dropped.iter().any(|d| d == what) {
        f.dropped.push(what.to_string());
    }
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
    // Mermaid's marks beside the arrow: a central connection's `()` at either
    // end, an activation's `+` or `-` before the target. The step is kept and
    // the ledger names each kind (#1537).
    let (from, to) = match (from.strip_suffix("()"), to.strip_prefix("()")) {
        (None, None) => (from, to),
        (a, b) => {
            record_drop(f, CENTRAL);
            (a.map_or(from, str::trim_end), b.map_or(to, str::trim_start))
        }
    };
    let to = match to.strip_prefix(['+', '-']) {
        Some(t) => {
            record_drop(f, ACTIVATIONS);
            t.trim_start()
        }
        None => to,
    };
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

/// A line's statements: a `;` ends one as a new line does, as in Mermaid,
/// except the `;` that closes an entity code such as `#59;` (a `#`, then
/// letters or digits). A `%%` line is one statement (#1537).
fn statements(l: &str) -> Vec<&str> {
    if l.starts_with("%%") {
        return vec![l];
    }
    let (mut out, mut from) = (vec![], 0);
    for (i, _) in l.match_indices(';') {
        let code = l[from..i].trim_end_matches(|c: char| c.is_ascii_alphanumeric());
        if !code.ends_with('#') || code.len() == i - from {
            out.push(&l[from..i]);
            from = i + 1;
        }
    }
    out.push(&l[from..]);
    out
}

/// Reads the subset in the module doc; an `Err` says why and on which line.
pub fn parse(src: &str) -> Result<Flow, String> {
    // Each statement with its line's number. A line's escape sequences go
    // first, whole, so a `;` in one ends no statement (#1530).
    let mut lines = vec![];
    for (n, l) in src.lines().enumerate() {
        let l = super::action::strip_escapes(l);
        lines.extend(statements(l.trim()).into_iter().map(|s| (n + 1, s.trim().to_string())));
    }
    let mut lines = lines.into_iter().filter(|(_, l)| !l.is_empty());
    match lines.find(|(_, l)| !l.starts_with("%%")) {
        Some((_, l)) if l == "sequenceDiagram" => {}
        _ => return Err("no sequenceDiagram header".into()),
    }
    let mut f = Flow::default();
    let mut open = vec![];
    for (n, l) in lines {
        // `#59;` is the one entity code read: a `;` in text.
        line(&mut f, &l.replace("#59;", ";"), &mut open).map_err(|why| format!("line {n}: {why}: {l}"))?;
    }
    if !open.is_empty() {
        return Err("a block is never closed".into());
    }
    if f.steps.is_empty() {
        return Err("no steps".into());
    }
    Ok(f)
}

/// The ladder's gutter (the current step's mark and the status), its time
/// column, and the slot each actor gets between them at width `w`, split
/// evenly and at least 2. [`shape`] picks by this slot and [`ladder`] draws in
/// it (#1538).
fn slots(f: &Flow, w: usize) -> (usize, usize, usize) {
    let gutter = if f.has_status() { 3 } else { 1 };
    let right = if f.steps.iter().any(|s| s.time.is_some()) { 6 } else { 0 };
    (gutter, right, (w.saturating_sub(gutter + right) / f.actors.len().max(1)).max(2))
}

/// Actor `i`'s lifeline column: the middle of its slot (#1538).
fn column(gutter: usize, slot: usize, i: usize) -> usize {
    gutter + slot * i + slot / 2
}

/// The columns of the sides of a frame `d` deep at width `w`, each two in
/// from the one around it: a row inside `d` frames keeps to the columns from
/// the first to the second, both in (#1536).
fn frame_sides(w: usize, gutter: usize, right: usize, d: usize) -> (usize, usize) {
    (gutter + 2 * d, w.saturating_sub(right + 1 + 2 * d))
}

/// The ladder at [`LADDER_MIN_WIDTH`] or more when each actor gets a slot of
/// [`LADDER_SLOT`] columns, the compact ladder when it gets [`COMPACT_SLOT`],
/// otherwise the list. The slot is the one the ladder draws in: the width
/// less the gutter and the time column, split evenly between the actors
/// (#1538). The list draws too where a frame's right side would sit on or
/// left of the last lifeline (#1578).
pub fn shape(f: &Flow, w: usize) -> Shape {
    let (gutter, right, slot) = slots(f, w);
    let last = column(gutter, slot, f.actors.len().saturating_sub(1));
    let clear = rules(f).iter().all(|(_, nest, _)| frame_sides(w, gutter, right, *nest).1 > last);
    if w < LADDER_MIN_WIDTH || slot < COMPACT_SLOT || !clear {
        Shape::List
    } else if slot < LADDER_SLOT {
        Shape::Compact
    } else {
        Shape::Ladder
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

/// The first lifeline column in `cols` right of column `c`, if any.
fn lifeline_after(cols: &[usize], c: usize) -> Option<usize> {
    cols.iter().copied().filter(|&l| l > c).min()
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
            let after = lifeline_after(cols, c);
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

/// Step `i`'s number under `autonumber`: the start, then the increment a step,
/// held at the largest number rather than wrapping (#1537).
fn number(f: &Flow, i: usize) -> Option<usize> {
    f.autonumber.map(|(start, by)| start.saturating_add(i.saturating_mul(by)))
}

fn label(f: &Flow, i: usize) -> String {
    match number(f, i) {
        Some(n) => format!("{n} {}", f.steps[i].label),
        None => f.steps[i].label.clone(),
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

/// The ledger: what the drawing leaves out, named on a line wrapped to the
/// width, which both shapes put after the last step's rows and notes and
/// before the NEED box; nothing when nothing was dropped (#1535).
fn ledger(f: &Flow, w: usize, g: &Glyphs) -> Vec<String> {
    if f.dropped.is_empty() {
        return vec![];
    }
    wrap(&format!("(dropped: {})", f.dropped.join(", ")), w, g)
}

/// A frame's rule row: its top with its block's keyword and label, a divider
/// with its block's divider word and its label, or its bottom (#1536).
enum Rule<'a> {
    Top(Block, &'a str),
    Divider(Block, &'a str),
    Bottom,
}

impl Rule<'_> {
    /// The rule's left end, fill and right end, and the tag two columns in;
    /// both shapes draw a rule from here.
    fn glyphs(&self, g: &Glyphs) -> ([&'static str; 3], String) {
        let b = g.plain;
        match self {
            Rule::Top(kind, label) => ([b.top_left, b.horizontal_top, b.top_right], format!("[{} {label}]", kind.word())),
            // A block built by hand with dividers it takes none of shows no word on them.
            Rule::Divider(kind, label) => (
                [g.frame_tee_left, g.dashed, g.frame_tee_right],
                format!("[{} {label}]", kind.divider().unwrap_or_default()),
            ),
            Rule::Bottom => ([b.bottom_left, b.horizontal_bottom, b.bottom_right], String::new()),
        }
    }
}

/// What both shapes draw, in order: a frame's rule row, a note before the
/// first step, a step, or a note after one.
enum Part<'a> {
    Rule(Rule<'a>),
    Lead(&'a Note),
    Step(usize, &'a Step),
    Note(&'a Note),
}

/// Each frame's rule rows in the order the source has them, with the spot
/// each sits at and how many frames are open around its frame: a frame opens
/// once the open frames it is not inside have closed and its parent's
/// dividers before it are drawn (#1536).
fn rules(f: &Flow) -> Vec<(Spot, usize, Rule<'_>)> {
    let mut out = vec![];
    // Each open frame, outermost first, and how many of its dividers are drawn.
    let mut open: Vec<(&Frame, usize)> = vec![];
    // After the last frame, `None` closes every frame still open.
    for next in f.frames.iter().map(Some).chain([None]) {
        while let Some(&(top, drawn)) = open.last() {
            let nest = open.len() - 1;
            let inside = next.filter(|n| n.depth > top.depth);
            // Before a frame inside it opens, the dividers before that frame; before it closes, all.
            let due: Vec<&(Spot, String)> = top
                .dividers
                .iter()
                .skip(drawn)
                .take_while(|(at, _)| inside.is_none_or(|n| *at <= n.open))
                .collect();
            let drawn = drawn + due.len();
            out.extend(due.into_iter().map(|(at, label)| (*at, nest, Rule::Divider(top.kind, label))));
            if inside.is_some() {
                if let Some(o) = open.last_mut() {
                    o.1 = drawn;
                }
                break;
            }
            out.push((top.close, nest, Rule::Bottom));
            open.pop();
        }
        if let Some(n) = next {
            out.push((n.open, open.len(), Rule::Top(n.kind, &n.label)));
            open.push((n, 0));
        }
    }
    out
}

/// The lead notes, the steps and their notes, with each frame's rule rows
/// where their spots put them, each with how many frames are open around it,
/// in the order both shapes draw them (#1536).
fn parts(f: &Flow) -> Vec<(Part<'_>, usize)> {
    // Each part and the spot it sits at: a step after every spot that names
    // it, and the end, with no part, after them all.
    let spot = |steps, notes| Spot { steps, notes };
    let mut items = vec![];
    for (k, n) in f.lead.iter().enumerate() {
        items.push((spot(0, k), Some(Part::Lead(n))));
    }
    for (i, s) in f.steps.iter().enumerate() {
        items.push((spot(i, usize::MAX), Some(Part::Step(i, s))));
        for (k, n) in s.notes.iter().enumerate() {
            items.push((spot(i + 1, k), Some(Part::Note(n))));
        }
    }
    items.push((spot(usize::MAX, usize::MAX), None));
    let mut pending = rules(f).into_iter().peekable();
    let (mut out, mut open) = (vec![], 0);
    for (at, part) in items {
        // The rule rows at or before this part: its frame is open after a top
        // or a divider, closed after a bottom.
        while let Some((_, nest, rule)) = pending.next_if(|(s, _, _)| *s <= at) {
            open = nest + usize::from(!matches!(rule, Rule::Bottom));
            out.push((Part::Rule(rule), nest));
        }
        out.extend(part.map(|p| (p, open)));
    }
    out
}

/// Actors across, two rows a step: the label, then the arrow under it.
fn ladder(f: &Flow, w: usize, g: &Glyphs) -> Vec<String> {
    let (gutter, right, slot) = slots(f, w);
    // One lifeline column an actor; a flow built by hand with no actor still has the first.
    let cols: Vec<usize> = (0..f.actors.len().max(1)).map(|i| column(gutter, slot, i)).collect();
    let x = |i: usize| lifeline(&cols, i);
    let sides = |d: usize| frame_sides(w, gutter, right, d);
    // The lifelines, and the sides of the `nest` frames around the row.
    let lifelines = |nest: usize| {
        let mut r = Row::new(w);
        for i in 0..f.actors.len() {
            r.put(x(i), g.plain.vertical_left);
        }
        for (left, end) in (0..nest).map(sides) {
            r.put(left, g.plain.vertical_left);
            r.put(end, g.plain.vertical_right);
        }
        r
    };
    // A note sits where its place says, inside the frames around it and left of the time column.
    let note = |out: &mut Vec<String>, n: &Note, nest: usize| {
        let (lo, last) = sides(nest);
        let (start, end) = note_region(&n.place, &cols, lo, last + 1);
        for row in note_rows(&n.text, end - start, g) {
            // Each row starts its region right of a lifeline, ends it left of one, and is centred over them.
            let spare = (end - start).saturating_sub(row.width());
            let pad = match n.place {
                Place::Right(_) => 0,
                Place::Left(_) => spare,
                Place::Over(..) => spare / 2,
            };
            let mut r = lifelines(nest);
            r.text(start + pad, &row);
            out.push(r.done());
        }
    };
    let mut head = Row::new(w);
    for (i, a) in f.actors.iter().enumerate() {
        let name = fit(&a.name, slot - 1, g);
        head.text(x(i).saturating_sub(name.width() / 2), &name);
    }
    let mut out = vec![head.done()];
    let cur = f.current();
    for (part, nest) in parts(f) {
        let (i, s) = match part {
            Part::Rule(rule) => {
                // The rule from side to side, a cross at each lifeline, then the tag two columns in,
                // so the tag wins over a cross.
                let ([left, fill, end], tag) = rule.glyphs(g);
                let (lo, hi) = sides(nest);
                let mut r = lifelines(nest);
                for c in lo + 1..hi {
                    r.put(c, fill);
                }
                r.put(lo, left);
                r.put(hi, end);
                for i in 0..f.actors.len() {
                    if (lo + 1..hi).contains(&x(i)) {
                        r.put(x(i), g.frame_cross);
                    }
                }
                r.text(lo + 2, &fit(&tag, hi.saturating_sub(lo + 3), g));
                out.push(r.done());
                continue;
            }
            Part::Lead(n) | Part::Note(n) => {
                note(&mut out, n, nest);
                continue;
            }
            Part::Step(i, s) => (i, s),
        };
        let (a, b) = (x(s.from).min(x(s.to)), x(s.from).max(x(s.to)));
        // The label and a self step's mark end before this column: a blank column before the next
        // lifeline, so they cover none, or past the last at the time column or the edge, or a blank
        // column before the innermost frame's side (#1534, #1536, #1558).
        let room_end = match lifeline_after(&cols, a) {
            Some(next) => next.saturating_sub(1),
            None => sides(nest).1 + 1,
        };
        let mut lab = lifelines(nest);
        if cur == Some(i) {
            lab.put(0, g.head_right);
        }
        if let Some(st) = s.status {
            lab.put(1, st.glyph(g));
        }
        // The label starts two columns after the step's left lifeline.
        lab.text(a + 2, &fit(&label(f, i), room_end.saturating_sub(a + 2), g));
        if let Some(t) = &s.time {
            lab.text(w - right + 1, &fit(t, right - 1, g));
        }
        out.push(lab.done());
        let mut arrow = lifelines(nest);
        let shaft = s.line.glyph(g);
        if a == b {
            let back = match s.head {
                Head::Filled | Head::Both => g.self_back,
                Head::Open => g.open_left,
                Head::Cross => g.head_cross,
                Head::None => "",
            };
            // The mark starts a column after the lifeline. Short of room it loses its shaft, then its
            // turn, so the head that tells its kind shows longest (#1558).
            let parts = [shaft, g.self_turn, back];
            let lost = (0..parts.len())
                .take_while(|&k| parts[k..].concat().width() > room_end.saturating_sub(a + 1))
                .count();
            arrow.text(a + 1, &parts[lost..].concat());
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
    }
    out.extend(ledger(f, w, g));
    out.extend(need_box(f, w, g));
    out
}

/// One numbered line a step: status, number, a rail for each frame around
/// it, label, who to whom, time; each frame's rule rows between (#1536).
fn list(f: &Flow, w: usize, g: &Glyphs) -> Vec<String> {
    let cur = f.current();
    let status = f.has_status();
    let here = format!("{} here", g.head_left);
    // The number column holds the largest number, the last step's (#1537).
    let digits = number(f, f.steps.len().saturating_sub(1)).map_or(0, |n| n.to_string().len().max(2));
    let parts = parts(f);
    // A line's label column holds its rails too, so who to whom still lines up.
    let longest = parts
        .iter()
        .filter_map(|(p, nest)| match p {
            Part::Step(_, s) => Some(nest + s.label.width()),
            _ => None,
        })
        .max()
        .unwrap_or(0);
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
    let lead_w = usize::from(status) * 2 + if f.autonumber.is_some() { digits + 1 } else { 0 };
    // One label column for every line: aligned when it holds the longest label
    // or 6 columns of it, whichever is less, and the rest of an aligned line
    // fits too (with every label empty the column holds them at any width);
    // compact otherwise. The time column and the gap before it take room only
    // when a step has a time.
    let time_col = if time_w > 0 { 2 + time_w } else { 0 };
    let rest_w = lead_w + 2 + who_w + time_col;
    let room = w.saturating_sub(rest_w).min(longest);
    let aligned = room >= longest.min(6) && rest_w <= w;
    let pad = " ".repeat(lead_w);
    let mut out = vec![];
    for (part, nest) in parts {
        // One rail a frame around the line, between its status and number and its label.
        let rails = g.plain.vertical_left.repeat(nest);
        let (i, s) = match part {
            Part::Rule(rule) => {
                let ([left, fill, _], tag) = rule.glyphs(g);
                out.push(format!("{pad}{rails}{left}{fill}{tag}"));
                continue;
            }
            // A note before the first step starts its line, as it did before frames.
            Part::Lead(n) if nest == 0 => {
                out.extend(note_rows(&n.text, w, g).iter().map(|l| fit(l, w, g)));
                continue;
            }
            Part::Lead(n) | Part::Note(n) => {
                for line in note_rows(&n.text, w.saturating_sub(lead_w + nest), g) {
                    out.push(fit(&format!("{pad}{rails}{line}"), w, g));
                }
                continue;
            }
            Part::Step(i, s) => (i, s),
        };
        let mut lead = String::new();
        if status {
            lead.push_str(s.status.map_or(" ", |st| st.glyph(g)));
            lead.push(' ');
        }
        if let Some(n) = number(f, i) {
            lead.push_str(&format!("{n:>digits$} "));
        }
        let who = who(s);
        let time = s.time.as_deref().unwrap_or("");
        let tail = format!("  {who}{}  {time}", " ".repeat(who_w - who.width()));
        let line = if aligned {
            let label = fit(&s.label, room.saturating_sub(nest), g);
            let gap = " ".repeat(room.saturating_sub(nest + label.width()));
            format!("{lead}{rails}{label}{gap}{tail}")
        } else {
            fit(format!("{lead}{rails}{} {who} {time}", s.label).trim_end(), w, g)
        };
        let line = line.trim_end();
        if cur == Some(i) && line.width() + 2 + here.width() <= w {
            out.push(format!("{line}  {here}"));
        } else {
            out.push(line.to_string());
            if cur == Some(i) {
                out.push(fit(&format!("{}{rails}{here}", " ".repeat(lead.width())), w, g));
            }
        }
    }
    out.extend(ledger(f, w, g));
    out.extend(need_box(f, w, g));
    out
}

/// `f` at width `w`, in the [`shape`] the width and the actors' slot pick,
/// with the glyphs of tier `g`. The compact ladder draws as the ladder does.
pub fn draw(f: &Flow, w: usize, g: &Glyphs) -> Vec<String> {
    let lines = match shape(f, w) {
        Shape::Ladder | Shape::Compact => ladder(f, w, g),
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
    use super::{
        Actor, Block, Flow, Frame, Glyphs, Head, Line, Note, Place, Row, Shape, Spot, Status, Step, draw, fit, ladder, list, parse, shape,
    };

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
            dropped: vec![format!("x{bad}y")],
            // Around the lead note and the step, its else before the step's note (#1536).
            frames: vec![Frame {
                kind: Block::Alt,
                label: format!("red{bad}x"),
                depth: 0,
                open: Spot { steps: 0, notes: 0 },
                dividers: vec![(Spot { steps: 1, notes: 0 }, format!("green{bad}y"))],
                close: Spot { steps: 1, notes: 1 },
            }],
            autonumber: Some((1, 1)),
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
                // The ledger and the frame are drawn too, their bad characters dropped (#1535, #1536).
                assert!(out.iter().any(|l| l.starts_with("(dropped: x")), "{row} w={w}: {out:?}");
                assert!(out.iter().any(|l| l.contains("[alt red")), "{row} w={w}: {out:?}");
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

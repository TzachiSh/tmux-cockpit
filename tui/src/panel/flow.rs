//! The flow renderer (#935): a subset of Mermaid's `sequenceDiagram` drawn as
//! text that fits a width, either a ladder (actors across, steps down) or a
//! numbered list. Pure: no tmux, no clock, no terminal.
//!
//! What it reads: `participant` and `actor` (with `as`), implicit participants,
//! the arrows `->>`, `-->>`, `->` and `-->`, `autonumber`, `Note`, and a
//! `%% status: <✓|●|○|✗|!|done|running|pending|failed|needs> [HH:MM]` line
//! after a step. `loop`, `alt`, `opt`, `else` and `end` are dropped and their
//! steps kept. Anything else is an `Err` naming the line, and [`render`] then
//! shows [`not_drawn`]: the reason and the raw lines.
//!
//! No line is ever wider than the width, and nothing here panics: the release
//! profile aborts on a panic.
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

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
    pub fn glyph(self) -> char {
        match self {
            Status::Done => '✓',
            Status::Running => '●',
            Status::Pending => '○',
            Status::Failed => '✗',
            Status::Needs => '!',
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

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Step {
    pub from: usize,
    pub to: usize,
    pub dashed: bool,
    pub label: String,
    pub status: Option<Status>,
    pub time: Option<String>,
    /// Notes written after this step, other than NEED FROM YOU.
    pub notes: Vec<String>,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Flow {
    pub actors: Vec<Actor>,
    pub steps: Vec<Step>,
    /// Notes before the first step.
    pub lead: Vec<String>,
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

/// Tabs become spaces and other control characters U+FFFD, so a width counts
/// what the terminal shows and no escape reaches it.
fn clean(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\t' => ' ',
            c if c.is_control() => '\u{FFFD}',
            c => c,
        })
        .collect()
}

fn is_id(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

fn is_time(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 5 && b[2] == b':' && [0, 1, 3, 4].iter().all(|&i| b[i].is_ascii_digit())
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

const ARROWS: [(&str, bool); 4] = [("-->>", true), ("->>", false), ("-->", true), ("->", false)];

/// One line, trimmed and not empty, into `f`. `depth` counts open blocks.
fn line(f: &mut Flow, l: &str, depth: &mut usize) -> Result<(), String> {
    if let Some(comment) = l.strip_prefix("%%") {
        if let Some(rest) = comment.trim_start().strip_prefix("status:") {
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
            f.actors[i].name = clean(name);
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
    let ids = ["left of ", "right of ", "over "]
        .iter()
        .find_map(|p| place.strip_prefix(p))
        .ok_or("a note without left of, right of or over")?;
    for id in ids.split(',').map(str::trim) {
        if !is_id(id) {
            return Err("a note over no participant".into());
        }
        f.actor(id);
    }
    let text = clean(text.trim());
    if let Some(need) = text.strip_prefix("NEED FROM YOU") {
        f.need.push(need.trim_start_matches(':').trim().to_string());
    } else if let Some(step) = f.steps.last_mut() {
        step.notes.push(text);
    } else {
        f.lead.push(text);
    }
    Ok(())
}

fn step(f: &mut Flow, l: &str) -> Result<(), String> {
    let (head, label) = l.split_once(':').ok_or("not a step")?;
    let (from, to, dashed) = ARROWS
        .iter()
        .find_map(|(a, dashed)| head.split_once(a).map(|(x, y)| (x.trim(), y.trim(), *dashed)))
        .ok_or("no arrow")?;
    if !is_id(from) || !is_id(to) {
        return Err("a step needs two participant ids".into());
    }
    let (from, to) = (f.actor(from), f.actor(to));
    f.steps.push(Step {
        from,
        to,
        dashed,
        label: clean(label.trim()),
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

/// Cut `s` to at most `w` columns, ending in `…` when cut. Cuts between
/// grapheme clusters, so an emoji sequence is kept whole or dropped whole.
fn fit(s: &str, w: usize) -> String {
    if s.width() <= w {
        return s.to_string();
    }
    if w == 0 {
        return String::new();
    }
    let mut kept: Vec<&str> = vec![];
    let mut used = 0;
    for g in s.graphemes(true) {
        if used + g.width() + 1 > w {
            break;
        }
        kept.push(g);
        used += g.width();
    }
    // Clusters can measure narrower apart than joined.
    while kept.concat().width() + 1 > w {
        kept.pop();
    }
    kept.concat() + "…"
}

/// Words into lines of at most `w` columns; a word wider than that is cut.
fn wrap(s: &str, w: usize) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    for word in s.split_whitespace() {
        if !cur.is_empty() && cur.width() + 1 + word.width() > w {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(&fit(word, w));
    }
    if !cur.is_empty() || out.is_empty() {
        out.push(cur);
    }
    out
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

/// The closing box of NEED FROM YOU notes, the width wide.
fn need_box(f: &Flow, w: usize) -> Vec<String> {
    if f.need.is_empty() {
        return vec![];
    }
    if w < 6 {
        return f.need.iter().map(|n| fit(&format!("NEED FROM YOU: {n}"), w)).collect();
    }
    let inner = w - 4;
    let title = fit(" NEED FROM YOU ", w - 2);
    let mut out = vec![format!("┌{title}{}┐", "─".repeat(w - 2 - title.width()))];
    for line in f.need.iter().flat_map(|n| wrap(n, inner)) {
        out.push(format!("│ {line}{} │", " ".repeat(inner.saturating_sub(line.width()))));
    }
    out.push(format!("└{}┘", "─".repeat(w - 2)));
    out
}

/// Actors across, two rows a step: the label, then the arrow under it.
fn ladder(f: &Flow, w: usize) -> Vec<String> {
    let gutter = if f.has_status() { 3 } else { 1 };
    let right = if f.steps.iter().any(|s| s.time.is_some()) { 6 } else { 0 };
    let slot = (w.saturating_sub(gutter + right) / f.actors.len().max(1)).max(2);
    let x = |i: usize| gutter + slot * i + slot / 2;
    let lifelines = || {
        let mut r = Row::new(w);
        for i in 0..f.actors.len() {
            r.put(x(i), "│");
        }
        r
    };
    let notes = |out: &mut Vec<String>, notes: &[String]| {
        for line in notes.iter().flat_map(|n| wrap(n, w.saturating_sub(gutter + 2))) {
            let mut r = lifelines();
            r.text(gutter, &format!("↳ {line}"));
            out.push(r.done());
        }
    };
    let mut head = Row::new(w);
    for (i, a) in f.actors.iter().enumerate() {
        let name = fit(&a.name, slot - 1);
        head.text(x(i).saturating_sub(name.width() / 2), &name);
    }
    let mut out = vec![head.done()];
    notes(&mut out, &f.lead);
    let cur = f.current();
    for (i, s) in f.steps.iter().enumerate() {
        let (a, b) = (x(s.from).min(x(s.to)), x(s.from).max(x(s.to)));
        let mut lab = lifelines();
        if cur == Some(i) {
            lab.put(0, "▶");
        }
        if let Some(st) = s.status {
            lab.put(1, st.glyph().encode_utf8(&mut [0; 4]));
        }
        lab.text(a + 2, &fit(&label(f, i), w.saturating_sub(right + a + 2)));
        if let Some(t) = &s.time {
            lab.text(w - right + 1, &fit(t, right - 1));
        }
        out.push(lab.done());
        let mut arrow = lifelines();
        if a == b {
            arrow.text(a + 1, "─╮↩");
        } else {
            for c in a + 1..b {
                arrow.put(c, if s.dashed { "┄" } else { "─" });
            }
            if s.to > s.from {
                arrow.put(b - 1, "▶");
            } else {
                arrow.put(a + 1, "◀");
            }
        }
        out.push(arrow.done());
        notes(&mut out, &s.notes);
    }
    out.extend(need_box(f, w));
    out
}

const HERE: &str = "◀ here";

/// One numbered line a step: status, number, label, who to whom, time.
fn list(f: &Flow, w: usize) -> Vec<String> {
    let cur = f.current();
    let status = f.has_status();
    let digits = if f.autonumber { f.steps.len().to_string().len().max(2) } else { 0 };
    let longest = f.steps.iter().map(|s| s.label.width()).max().unwrap_or(0);
    let who = |s: &Step| format!("{}→{}", f.actors[s.from].name, f.actors[s.to].name);
    let who_w = f.steps.iter().map(|s| who(s).width()).max().unwrap_or(0);
    let mut out = vec![];
    for (i, s) in f.steps.iter().enumerate() {
        let mut lead = String::new();
        if status {
            lead.push(s.status.map_or(' ', Status::glyph));
            lead.push(' ');
        }
        if f.autonumber {
            lead.push_str(&format!("{:>digits$} ", i + 1));
        }
        let who = who(s);
        let time = s.time.as_deref().unwrap_or("");
        let tail = format!("  {who}{}  {time}", " ".repeat(who_w - who.width()));
        let room = w.saturating_sub(lead.width() + tail.width()).min(longest);
        let line = if room >= 6 {
            let label = fit(&s.label, room);
            format!("{lead}{label}{}{tail}", " ".repeat(room - label.width()))
        } else {
            fit(&format!("{lead}{} {who} {time}", s.label), w)
        };
        let line = line.trim_end();
        if cur == Some(i) && line.width() + 2 + HERE.width() <= w {
            out.push(format!("{line}  {HERE}"));
        } else {
            out.push(line.to_string());
            if cur == Some(i) {
                out.push(fit(&format!("{}{HERE}", " ".repeat(lead.width())), w));
            }
        }
        let indent = " ".repeat(lead.width());
        for line in s.notes.iter().flat_map(|n| wrap(n, w.saturating_sub(lead.width() + 2))) {
            out.push(fit(&format!("{indent}↳ {line}"), w));
        }
    }
    let mut head: Vec<String> = f
        .lead
        .iter()
        .flat_map(|n| wrap(n, w.saturating_sub(2)))
        .map(|l| fit(&format!("↳ {l}"), w))
        .collect();
    head.extend(out);
    head.extend(need_box(f, w));
    head
}

/// `f` at width `w`, in the [`shape`] the width and actor count pick.
pub fn draw(f: &Flow, w: usize) -> Vec<String> {
    let lines = match shape(f, w) {
        Shape::Ladder => ladder(f, w),
        Shape::List => list(f, w),
    };
    // The layouts aim to fit; this makes it a guarantee.
    lines.into_iter().map(|l| fit(&l, w)).collect()
}

/// What the caller shows for source that does not parse: the reason, then the
/// raw lines, each cut to the width.
pub fn not_drawn(src: &str, why: &str, w: usize) -> Vec<String> {
    std::iter::once(fit(&clean(&format!("(not drawn: {why})")), w))
        .chain(src.lines().map(|l| fit(&clean(l), w)))
        .collect()
}

/// [`parse`] then [`draw`], or [`not_drawn`] when it does not parse.
pub fn render(src: &str, w: usize) -> Vec<String> {
    match parse(src) {
        Ok(f) => draw(&f, w),
        Err(why) => not_drawn(src, &why, w),
    }
}

//! Markdown to rows (#283): pulldown-cmark reads the text, this lays it out at
//! one width. Pure: no theme and no terminal, so a test asserts on a row's
//! cells. No row is wider than the width it was laid out at, measured per
//! grapheme as the buffer draws it. Nothing in the text is run or followed:
//! a link is its text and its URL, an HTML tag is shown as written.
use crate::highlight::{self, Role};
use crate::pick::clean;
use pulldown_cmark::{Alignment, CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// What a cell is. The screen maps each to an `@thm_*` colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Text,
    /// a heading's text, by level 1 to 6
    Heading(u8),
    Quote,
    /// a code block's token, by the lexer's role
    Code(Role),
    /// `inline code`
    Inline,
    /// a link's text
    Link,
    /// a link's or an image's address, shown after it
    Url,
    /// a list's bullet, number or task box
    Bullet,
    /// a quote bar, a table border, a rule
    Frame,
    /// a heading's `#`s, raw HTML, an image's label
    Meta,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ink {
    pub kind: Kind,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
}

impl Ink {
    pub const fn of(kind: Kind) -> Self {
        Ink {
            kind,
            bold: false,
            italic: false,
            strike: false,
        }
    }
}

/// One drawn row: its cells in order, and the source line (from 1) of the
/// block it came from, which `e` opens the editor at.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Row {
    pub cells: Vec<(String, Ink)>,
    pub line: usize,
    /// the bytes of `cells` a gutter takes (a code row's line number): drawn,
    /// never searched. 0 for markdown.
    pub lead: usize,
}

impl Row {
    pub fn text(&self) -> String {
        self.cells.iter().map(|(t, _)| t.as_str()).collect()
    }

    /// The text find reads: the gutter's bytes as NULs, which no query holds,
    /// so a match never lands on a line number and keeps its byte offsets.
    pub fn body(&self) -> String {
        let t = self.text();
        let lead = self.lead.min(t.len());
        format!("{}{}", "\0".repeat(lead), &t[lead..])
    }

    pub fn width(&self) -> usize {
        self.cells.iter().map(|(t, _)| cols(t)).sum()
    }
}

/// How to draw: the glyph tier, whether a link's URL is shown, and whether
/// there is colour (without it inline code keeps its backticks).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opts {
    pub ascii: bool,
    pub urls: bool,
    pub colour: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            ascii: false,
            urls: true,
            colour: true,
        }
    }
}

/// Columns as the buffer draws them: per grapheme, so ❤️ is 2.
pub fn cols(s: &str) -> usize {
    s.graphemes(true).map(UnicodeWidthStr::width).sum()
}

/// A code line's tab stop: the panel's 4.
const TAB: usize = 4;
/// A code block's indent, as markdown's own.
const CODE_INDENT: &str = "    ";
/// The columns a table's separator takes: ` │ `.
const SEP: usize = 3;
/// Narrower than this a table column is unreadable: the table is drawn as
/// `header: value` lines instead.
const MIN_COL: usize = 3;

type Cells = Vec<(String, Ink)>;

fn push(cells: &mut Cells, s: &str, ink: Ink) {
    if s.is_empty() {
        return;
    }
    match cells.last_mut() {
        Some((t, i)) if *i == ink => t.push_str(s),
        _ => cells.push((s.to_string(), ink)),
    }
}

fn width_of(cells: &[(String, Ink)]) -> usize {
    cells.iter().map(|(t, _)| cols(t)).sum()
}

/// The greedy wrapper's state: the rows so far, the row being filled, and the
/// spaces and the word waiting to go on it.
struct Wrap {
    w: usize,
    rows: Vec<Cells>,
    row: Cells,
    rw: usize,
    space: Cells,
    sw: usize,
    word: Cells,
    ww: usize,
}

impl Wrap {
    /// The waiting word onto the row after its spaces, or onto a new row
    /// without them; a word wider than a row is cut between graphemes.
    fn place(&mut self) {
        if self.word.is_empty() {
            return;
        }
        if self.rw + self.sw + self.ww <= self.w {
            for (t, ink) in self.space.drain(..).chain(self.word.drain(..)) {
                push(&mut self.row, &t, ink);
            }
            self.rw += self.sw + self.ww;
        } else {
            if self.rw > 0 {
                self.rows.push(std::mem::take(&mut self.row));
            }
            self.rw = 0;
            for (t, ink) in std::mem::take(&mut self.word) {
                for g in t.graphemes(true) {
                    let gw = cols(g);
                    if self.rw + gw > self.w && self.rw > 0 {
                        self.rows.push(std::mem::take(&mut self.row));
                        self.rw = 0;
                    }
                    push(&mut self.row, g, ink);
                    self.rw += gw;
                }
            }
        }
        self.space.clear();
        (self.sw, self.ww) = (0, 0);
    }

    fn feed(&mut self, t: &str, ink: Ink) {
        for g in t.graphemes(true) {
            if g == "\n" {
                self.place();
                self.rows.push(std::mem::take(&mut self.row));
                self.space.clear();
                (self.rw, self.sw) = (0, 0);
            } else if g == " " {
                self.place();
                push(&mut self.space, g, ink);
                self.sw += 1;
            } else {
                push(&mut self.word, g, ink);
                self.ww += cols(g);
            }
        }
    }
}

/// Greedy word wrap into rows `w` wide. A space at a break is dropped, a word
/// wider than a row is cut between graphemes, and a `\n` cell breaks the row.
fn wrap(spans: &[(String, Ink)], w: usize) -> Vec<Cells> {
    let mut s = Wrap {
        w: w.max(2),
        rows: Vec::new(),
        row: Cells::new(),
        rw: 0,
        space: Cells::new(),
        sw: 0,
        word: Cells::new(),
        ww: 0,
    };
    for (t, ink) in spans {
        s.feed(t, *ink);
    }
    s.place();
    if !s.row.is_empty() || s.rows.is_empty() {
        s.rows.push(s.row);
    }
    s.rows
}

/// Rows `w` wide cut between graphemes, every space kept: a code line.
pub(super) fn hard(spans: &[(String, Ink)], w: usize) -> Vec<Cells> {
    let w = w.max(2);
    let mut rows = vec![Cells::new()];
    let mut rw = 0;
    for (t, ink) in spans {
        for g in t.graphemes(true) {
            let gw = cols(g);
            if rw + gw > w && rw > 0 {
                rows.push(Cells::new());
                rw = 0;
            }
            push(rows.last_mut().expect("never empty"), g, *ink);
            rw += gw;
        }
    }
    rows
}

/// A code line's spans with each tab out to the next stop and every other
/// control character dropped.
pub(super) fn untab(spans: &[(Role, &str)]) -> Cells {
    let mut out = Cells::new();
    let mut col = 0;
    for (r, t) in spans {
        let mut s = String::with_capacity(t.len());
        for g in t.graphemes(true) {
            if g == "\t" {
                let n = TAB - col % TAB;
                s.extend(std::iter::repeat_n(' ', n));
                col += n;
            } else {
                let g = clean(g);
                col += cols(&g);
                s.push_str(&g);
            }
        }
        push(&mut out, &s, Ink::of(Kind::Code(*r)));
    }
    out
}

/// A container a row is drawn inside: a quote's bar, or a list item's bullet
/// (drawn once, on the item's first row) and the indent under it.
enum Frame {
    Quote,
    Item { bullet: Option<String>, width: usize },
}

struct Table {
    align: Vec<Alignment>,
    /// each row's cells and source line; the first row is the head
    rows: Vec<(Vec<Cells>, usize)>,
}

struct Build {
    o: Opts,
    w: usize,
    starts: Vec<usize>,
    out: Vec<Row>,
    frames: Vec<Frame>,
    /// each open list's next number, `None` for a bulleted one
    lists: Vec<Option<u64>>,
    inline: Cells,
    bold: u32,
    italic: u32,
    strike: u32,
    links: Vec<(String, bool)>,
    /// each open image's address
    images: Vec<String>,
    heading: Option<u8>,
    quotes: u32,
    /// a blank row is owed before the next block
    gap: bool,
    line: usize,
    /// an open code or HTML block: its tag (`None` for HTML), first line, text
    code: Option<(Option<String>, usize, String)>,
    table: Option<Table>,
}

impl Build {
    fn line_of(&self, off: usize) -> usize {
        self.starts.partition_point(|&s| s <= off)
    }

    fn ink(&self) -> Ink {
        let kind = if !self.links.is_empty() {
            Kind::Link
        } else if !self.images.is_empty() {
            Kind::Meta
        } else if let Some(l) = self.heading {
            Kind::Heading(l)
        } else if self.quotes > 0 {
            Kind::Quote
        } else {
            Kind::Text
        };
        let head = self.table.as_ref().is_some_and(|t| t.rows.len() == 1);
        Ink {
            kind,
            bold: self.bold > 0 || self.heading.is_some() || head,
            italic: self.italic > 0,
            strike: self.strike > 0,
        }
    }

    fn text(&mut self, s: &str) {
        let ink = self.ink();
        let s = clean(&s.replace('\n', " "));
        push(&mut self.inline, &s, ink);
    }

    /// The prefix's columns, at most half the width: deep nesting gives up its
    /// bullets before the text gives up its room.
    fn indent(&self) -> usize {
        let full: usize = self
            .frames
            .iter()
            .map(|f| if let Frame::Item { width, .. } = f { *width } else { 2 })
            .sum();
        full.min(self.w / 2)
    }

    /// The prefix for the next row, clamped to [`indent`]. `first` takes each
    /// pending bullet; a blank row shows only the bars.
    fn prefix(&mut self, first: bool) -> Cells {
        let (ascii, max) = (self.o.ascii, self.indent());
        let mut out = Cells::new();
        for f in &mut self.frames {
            match f {
                Frame::Quote => push(&mut out, if ascii { "| " } else { "│ " }, Ink::of(Kind::Frame)),
                Frame::Item { bullet, width } => match if first { bullet.take() } else { None } {
                    Some(b) => push(&mut out, &b, Ink::of(Kind::Bullet)),
                    None => push(&mut out, &" ".repeat(*width), Ink::default()),
                },
            }
        }
        let mut cut = Cells::new();
        let mut used = 0;
        for (t, ink) in out {
            for g in t.graphemes(true) {
                if used + cols(g) > max {
                    break;
                }
                used += cols(g);
                push(&mut cut, g, ink);
            }
        }
        push(&mut cut, &" ".repeat(max - used), Ink::default());
        cut
    }

    /// The blank row a block owes, drawn under the frames open now: called
    /// before a quote or an item opens, the gap is outside it. It carries the
    /// line of the row above, so a line's first row is never a blank one.
    fn settle(&mut self) {
        if std::mem::take(&mut self.gap)
            && let Some(line) = self.out.last().map(|r| r.line)
        {
            let mut cells = self.prefix(false);
            while let Some((t, _)) = cells.last_mut() {
                let k = t.trim_end().len();
                t.truncate(k);
                if !t.is_empty() {
                    break;
                }
                cells.pop();
            }
            self.out.push(Row { cells, line, lead: 0 });
        }
    }

    /// Content rows, each under the prefix, after the blank row a block owes.
    fn emit(&mut self, rows: Vec<Cells>, line: usize) {
        if rows.is_empty() {
            return;
        }
        self.settle();
        for (i, r) in rows.into_iter().enumerate() {
            let mut cells = self.prefix(i == 0);
            cells.extend(r);
            self.out.push(Row { cells, line, lead: 0 });
        }
    }

    /// The paragraph so far, wrapped and drawn.
    fn flush(&mut self) {
        let spans = std::mem::take(&mut self.inline);
        if spans.iter().all(|(t, _)| t.trim().is_empty()) {
            return;
        }
        let rows = wrap(&spans, self.w - self.indent());
        self.emit(rows, self.line);
    }

    fn code(&mut self) {
        let Some((tag, first, body)) = self.code.take() else { return };
        let body = body.strip_suffix('\n').unwrap_or(&body);
        let lines: Vec<&str> = body.split('\n').collect();
        let (lexed, pad) = match &tag {
            Some(t) => (highlight::block(t, &lines), CODE_INDENT),
            None => (lines.iter().map(|l| vec![(Role::Plain, *l)]).collect(), ""),
        };
        let room = self.w - self.indent();
        let pad = if room > 2 * pad.len() { pad } else { "" };
        for (i, spans) in lexed.iter().enumerate() {
            let mut cells = untab(spans);
            if tag.is_none() {
                for c in &mut cells {
                    c.1 = Ink::of(Kind::Meta);
                }
            }
            let rows: Vec<Cells> = hard(&cells, room - pad.len())
                .into_iter()
                .map(|r| {
                    let mut c = Cells::new();
                    push(&mut c, pad, Ink::default());
                    c.extend(r);
                    c
                })
                .collect();
            self.emit(rows, first + i);
        }
        self.gap = true;
    }

    fn rule(&mut self, line: usize) {
        let room = self.w - self.indent();
        let dash = if self.o.ascii { "-" } else { "─" };
        let mut c = Cells::new();
        push(&mut c, &dash.repeat(room), Ink::of(Kind::Frame));
        self.emit(vec![c], line);
        self.gap = true;
    }

    fn table(&mut self) {
        let Some(t) = self.table.take() else { return };
        let n = t.rows.iter().map(|(c, _)| c.len()).max().unwrap_or(0);
        let room = self.w - self.indent();
        if n == 0 {
            return;
        }
        if n * MIN_COL + SEP * (n - 1) > room {
            return self.table_as_lines(t, room);
        }
        let mut nat = vec![1; n];
        for (cells, _) in &t.rows {
            for (i, c) in cells.iter().enumerate() {
                nat[i] = nat[i].max(width_of(c));
            }
        }
        let cw = shares(&nat, room - SEP * (n - 1));
        let (bar, dash, cross) = if self.o.ascii {
            (" | ", "-", "-+-")
        } else {
            (" │ ", "─", "─┼─")
        };
        let frame = Ink::of(Kind::Frame);
        for (r, (cells, line)) in t.rows.iter().enumerate() {
            let wrapped: Vec<Vec<Cells>> = (0..n)
                .map(|i| cells.get(i).map_or_else(|| vec![Cells::new()], |c| wrap(c, cw[i])))
                .collect();
            let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
            let mut rows = Vec::with_capacity(height);
            for k in 0..height {
                let mut row = Cells::new();
                for (i, col) in wrapped.iter().enumerate() {
                    if i > 0 {
                        push(&mut row, bar, frame);
                    }
                    let cell = col.get(k).cloned().unwrap_or_default();
                    let room = cw[i].saturating_sub(width_of(&cell));
                    let left = match t.align.get(i) {
                        Some(Alignment::Right) => room,
                        Some(Alignment::Center) => room / 2,
                        _ => 0,
                    };
                    push(&mut row, &" ".repeat(left), Ink::default());
                    row.extend(cell);
                    // the last column's padding would only be trailing space
                    if i + 1 < n {
                        push(&mut row, &" ".repeat(room - left), Ink::default());
                    }
                }
                rows.push(row);
            }
            if r == 0 {
                let mut sep = Cells::new();
                let line = cw.iter().map(|w| dash.repeat(*w)).collect::<Vec<_>>().join(cross);
                push(&mut sep, &line, frame);
                rows.push(sep);
            }
            self.emit(rows, *line);
        }
        self.gap = true;
    }

    /// A table too wide for its columns: each body row as `header: value` lines.
    fn table_as_lines(&mut self, t: Table, room: usize) {
        let mut rows = t.rows.into_iter();
        let head = rows.next().map(|(c, _)| c).unwrap_or_default();
        for (cells, line) in rows {
            let mut out = Vec::new();
            for (i, c) in cells.into_iter().enumerate() {
                let mut spans = Cells::new();
                for (s, ink) in head.get(i).cloned().unwrap_or_default() {
                    push(&mut spans, &s, Ink { bold: true, ..ink });
                }
                push(&mut spans, ": ", Ink::of(Kind::Meta));
                spans.extend(c);
                out.extend(wrap(&spans, room));
            }
            self.emit(out, line);
            self.gap = true;
        }
    }

    fn start(&mut self, tag: Tag, off: usize) {
        let line = self.line_of(off);
        match tag {
            Tag::Paragraph => self.line = line,
            Tag::Heading { level, .. } => {
                self.flush();
                self.line = line;
                let n = level as u8;
                self.heading = Some(n);
                push(&mut self.inline, &format!("{} ", "#".repeat(n.into())), Ink::of(Kind::Meta));
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.settle();
                self.frames.push(Frame::Quote);
                self.quotes += 1;
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                let (tag, first) = match kind {
                    CodeBlockKind::Fenced(info) => (highlight::tag(&info).to_string(), line + 1),
                    CodeBlockKind::Indented => (String::new(), line),
                };
                self.code = Some((Some(tag), first, String::new()));
            }
            Tag::HtmlBlock => {
                self.flush();
                self.code = Some((None, line, String::new()));
            }
            Tag::List(start) => {
                self.flush();
                self.lists.push(start);
            }
            Tag::Item => {
                self.flush();
                self.settle();
                self.line = line;
                let depth = self.lists.len().saturating_sub(1);
                let bullet = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        *n += 1;
                        format!("{}. ", *n - 1)
                    }
                    _ => {
                        let marks = if self.o.ascii { ["-", "*", "+"] } else { ["•", "◦", "▪"] };
                        format!("{} ", marks[depth % 3])
                    }
                };
                let width = cols(&bullet);
                self.frames.push(Frame::Item {
                    bullet: Some(bullet),
                    width,
                });
            }
            Tag::Table(align) => {
                self.flush();
                self.table = Some(Table { align, rows: Vec::new() });
            }
            Tag::TableHead | Tag::TableRow => {
                if let Some(t) = &mut self.table {
                    t.rows.push((Vec::new(), line));
                }
            }
            Tag::TableCell => self.inline.clear(),
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { link_type, dest_url, .. } => {
                let auto = matches!(link_type, LinkType::Autolink | LinkType::Email);
                self.links.push((clean(&dest_url), auto));
            }
            Tag::Image { dest_url, .. } => {
                push(&mut self.inline, "[image: ", Ink::of(Kind::Meta));
                self.images.push(clean(&dest_url));
            }
            _ => {}
        }
    }

    /// A link's or an image's address after its text: dimmed, and only when it
    /// says something the text does not (an autolink is its own text).
    fn url(&mut self, url: &str, auto: bool) {
        if self.o.urls && !auto && !url.is_empty() && !url.starts_with('#') {
            push(&mut self.inline, &format!(" ({url})"), Ink::of(Kind::Url));
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush();
                self.gap = true;
            }
            TagEnd::Heading(_) => {
                self.flush();
                self.heading = None;
                self.gap = true;
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.frames.pop();
                self.quotes = self.quotes.saturating_sub(1);
                self.gap = true;
            }
            TagEnd::CodeBlock | TagEnd::HtmlBlock => self.code(),
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
                if self.lists.is_empty() {
                    self.gap = true;
                }
            }
            TagEnd::Item => {
                self.flush();
                self.frames.pop();
            }
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.inline);
                if let Some((cells, _)) = self.table.as_mut().and_then(|t| t.rows.last_mut()) {
                    cells.push(cell);
                }
            }
            TagEnd::Table => self.table(),
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link => {
                if let Some((url, auto)) = self.links.pop() {
                    self.url(&url, auto);
                }
            }
            TagEnd::Image => {
                push(&mut self.inline, "]", Ink::of(Kind::Meta));
                if let Some(url) = self.images.pop() {
                    self.url(&url, false);
                }
            }
            _ => {}
        }
    }

    fn event(&mut self, ev: Event, off: usize) {
        match ev {
            Event::Start(t) => self.start(t, off),
            Event::End(t) => self.end(t),
            Event::Text(t) | Event::Html(t) if self.code.is_some() => {
                if let Some((_, _, body)) = &mut self.code {
                    body.push_str(&t);
                }
            }
            Event::Text(t) | Event::InlineMath(t) | Event::DisplayMath(t) | Event::FootnoteReference(t) => self.text(&t),
            Event::Html(t) | Event::InlineHtml(t) => {
                let s = clean(&t.replace('\n', " "));
                push(&mut self.inline, &s, Ink::of(Kind::Meta));
            }
            Event::Code(t) => {
                let t = clean(&t);
                let t = if self.o.colour { t } else { format!("`{t}`") };
                let ink = Ink {
                    kind: Kind::Inline,
                    ..self.ink()
                };
                push(&mut self.inline, &t, ink);
            }
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.inline.push(("\n".into(), Ink::default())),
            Event::Rule => {
                self.flush();
                self.rule(self.line_of(off));
            }
            Event::TaskListMarker(done) => {
                let ascii = self.o.ascii;
                if let Some(Frame::Item {
                    bullet: bullet @ Some(_),
                    width,
                }) = self.frames.last_mut()
                {
                    let b = match (done, ascii) {
                        (false, _) => "[ ] ",
                        (true, false) => "[✓] ",
                        (true, true) => "[x] ",
                    };
                    *bullet = Some(b.to_string());
                    *width = cols(b);
                }
            }
        }
    }
}

/// Columns for a table's natural widths that sum to at most `room`: a column
/// that fits its fair share keeps its width, and the rest split what is left.
fn shares(nat: &[usize], room: usize) -> Vec<usize> {
    if nat.iter().sum::<usize>() <= room {
        return nat.to_vec();
    }
    let mut cw = nat.to_vec();
    let mut open: Vec<usize> = (0..nat.len()).collect();
    let mut left = room;
    loop {
        let share = left / open.len().max(1);
        let (fit, wide): (Vec<usize>, Vec<usize>) = open.iter().partition(|&&i| nat[i] <= share);
        if fit.is_empty() {
            let extra = left - share * wide.len();
            for (k, i) in wide.iter().enumerate() {
                cw[*i] = share + usize::from(k < extra);
            }
            return cw;
        }
        left -= fit.iter().map(|&i| nat[i]).sum::<usize>();
        open = wide;
        if open.is_empty() {
            return cw;
        }
    }
}

/// The markdown's rows at `w` columns. CommonMark with tables, task lists and
/// strikethrough; anything else (footnotes, math) is its text.
pub fn render(src: &str, w: usize, o: Opts) -> Vec<Row> {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH;
    let mut starts = vec![0];
    starts.extend(src.match_indices('\n').map(|(i, _)| i + 1));
    let mut b = Build {
        o,
        w: w.max(4),
        starts,
        out: Vec::new(),
        frames: Vec::new(),
        lists: Vec::new(),
        inline: Cells::new(),
        bold: 0,
        italic: 0,
        strike: 0,
        links: Vec::new(),
        images: Vec::new(),
        heading: None,
        quotes: 0,
        gap: false,
        line: 1,
        code: None,
        table: None,
    };
    for (ev, r) in Parser::new_ext(src, opts).into_offset_iter() {
        b.event(ev, r.start);
    }
    b.flush();
    b.code();
    b.out
}

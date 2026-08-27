//! The colour roles of screens.md §0 and the glyph tier, read once at startup.
//! tmux translates outward per attached client (truecolor is what gets
//! downgraded, 256 survives any 256-colour client), so there is no 16 tier and
//! nothing here ever queries the terminal.
use crate::data::tmux;
use ratatui::style::Color;
use ratatui::symbols::border;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, PartialEq)]
pub struct Theme {
    pub label: Color,
    pub value: Color,
    pub quote: Color,
    pub meta: Color,
    pub accent: Color,
    pub needs: Color,
    pub error: Color,
    pub running: Color,
    pub done: Color,
    /// the review mark: ◆ must not share "working" yellow with ● (chrome.md C1)
    pub sky: Color,
    pub sel_bg: Color,
    pub g: Glyphs,
}

/// role -> (@thm option, Catppuccin Mocha default)
const ROLES: [(&str, &str); 10] = [
    ("@thm_overlay_1", "#7f849c"),
    ("@thm_subtext_0", "#a6adc8"),
    ("@thm_overlay_0", "#6c7086"),
    ("@thm_lavender", "#b4befe"),
    ("@thm_red", "#f38ba8"),
    ("@thm_peach", "#fab387"),
    ("@thm_yellow", "#f9e2af"),
    ("@thm_green", "#a6e3a1"),
    ("@thm_sky", "#89dceb"),
    ("@thm_surface_0", "#313244"),
];

fn hex(s: &str) -> Option<Color> {
    let h = s.trim().trim_matches('"').trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(h, 16).ok()?;
    Some(Color::Rgb((n >> 16) as u8, (n >> 8) as u8, n as u8))
}

impl Theme {
    /// One tmux call, once per process.
    pub fn load() -> Self {
        if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
            return Self {
                g: Glyphs::from_env(),
                ..Self::no_color()
            };
        }
        let opts = tmux(&["show-options", "-g"]).unwrap_or_default();
        let mut c = [Color::Reset; 10];
        for (i, (opt, fallback)) in ROLES.iter().enumerate() {
            let set = opts.lines().find_map(|l| l.strip_prefix(opt)?.strip_prefix(' '));
            c[i] = set.and_then(hex).or_else(|| hex(fallback)).unwrap_or(Color::Reset);
        }
        Self {
            g: Glyphs::from_env(),
            ..Self::from_roles(c)
        }
    }

    fn from_roles(c: [Color; 10]) -> Self {
        Self {
            label: c[0],
            value: Color::Reset,
            quote: c[1],
            meta: c[2],
            accent: c[3],
            needs: c[4],
            error: c[5],
            running: c[6],
            done: c[7],
            sky: c[8],
            sel_bg: c[9],
            g: Glyphs::unicode(),
        }
    }

    /// The test fixture: the shipped defaults with no tmux read, so a snapshot
    /// never moves with the user's palette.
    pub fn fixed() -> Self {
        let mut c = [Color::Reset; 10];
        for (i, (_, fallback)) in ROLES.iter().enumerate() {
            c[i] = hex(fallback).unwrap_or(Color::Reset);
        }
        Self::from_roles(c)
    }

    /// NO_COLOR, honoured in-app: crossterm's own support emits an empty SGR
    /// that also drops bold, dim and reverse — the three attributes the
    /// colourless fallback is built on (p2pmux tui/terminal.rs:79-99).
    pub fn no_color() -> Self {
        Self::from_roles([Color::Reset; 10])
    }

    /// Per-state glyph and colour. chrome.md §1's urgency ramp.
    pub fn glyph(&self, state: &str) -> (&'static str, Color) {
        match state {
            "needs-input" => (self.g.needs, self.needs),
            "error" => (self.g.error, self.error),
            "running" => (self.g.running, self.running),
            "done" => (self.g.done, self.done),
            _ => (self.g.idle, self.meta),
        }
    }

    pub fn sel(&self) -> ratatui::style::Style {
        ratatui::style::Style::new()
            .bg(self.sel_bg)
            .add_modifier(ratatui::style::Modifier::BOLD)
    }

    pub fn review_mark(&self, review: &str) -> (&'static str, Color) {
        match review {
            "pending" => (self.g.review_pending, self.sky),
            "sent" | "pasted" => (self.g.review_sent, self.meta),
            _ => (" ", self.meta),
        }
    }
}

/// The glyph tier. `COCKPIT_GLYPHS=ascii` swaps glyphs *and* border sets,
/// because `BorderType::Plain` is box-drawing; the ellipsis goes from one cell
/// to three, which every width budget has to subtract.
#[derive(Clone, Copy, PartialEq)]
pub struct Glyphs {
    pub needs: &'static str,
    pub error: &'static str,
    pub running: &'static str,
    pub done: &'static str,
    pub idle: &'static str,
    pub review_pending: &'static str,
    pub review_sent: &'static str,
    pub active: &'static str,
    pub enter: &'static str,
    pub ellipsis: &'static str,
    pub dot: &'static str,
    pub dash: &'static str,
    pub up: &'static str,
    pub down: &'static str,
    pub plain: border::Set<'static>,
    pub thick: border::Set<'static>,
    pub round: border::Set<'static>,
}

const ASCII_PLAIN: border::Set = border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

const ASCII_THICK: border::Set = border::Set {
    top_left: "#",
    top_right: "#",
    bottom_left: "#",
    bottom_right: "#",
    vertical_left: "#",
    vertical_right: "#",
    horizontal_top: "=",
    horizontal_bottom: "=",
};

impl Glyphs {
    pub fn from_env() -> Self {
        if std::env::var("COCKPIT_GLYPHS").as_deref() == Ok("ascii") {
            Self::ascii()
        } else {
            Self::unicode()
        }
    }

    pub const fn unicode() -> Self {
        Self {
            needs: "▲",
            error: "✕",
            running: "●",
            done: "✓",
            idle: "○",
            review_pending: "◆",
            review_sent: "↵",
            active: "▎",
            enter: "↵",
            ellipsis: "…",
            dot: "·",
            dash: "—",
            up: "▲",
            down: "▼",
            plain: border::PLAIN,
            thick: border::THICK,
            round: border::ROUNDED,
        }
    }

    pub const fn ascii() -> Self {
        Self {
            needs: "!",
            error: "x",
            running: "*",
            done: "+",
            idle: ".",
            review_pending: "#",
            review_sent: "<",
            active: ">",
            enter: "<",
            ellipsis: "...",
            dot: ".",
            dash: "-",
            up: "^",
            down: "v",
            plain: ASCII_PLAIN,
            thick: ASCII_THICK,
            round: ASCII_PLAIN,
        }
    }

    /// Right truncation: the head identifies a title. Budgets subtract the
    /// ellipsis's own width, three cells in the ascii tier.
    pub fn trunc(&self, s: &str, w: usize) -> String {
        if s.width() <= w {
            return s.to_string();
        }
        let e = self.ellipsis.width();
        if w <= e {
            return head(s, w);
        }
        format!("{}{}", head(s, w - e), self.ellipsis)
    }

    /// Left truncation: the tail identifies a path or a `repo⑂branch`.
    pub fn trunc_left(&self, s: &str, w: usize) -> String {
        if s.width() <= w {
            return s.to_string();
        }
        let e = self.ellipsis.width();
        if w <= e {
            return head(s, w);
        }
        format!("{}{}", self.ellipsis, tail(s, w - e))
    }

    /// Middle truncation for the tool line: `Bash: shellcheck on cockpit.tmux
    /// and tests/run.sh` loses its meaning from either end alone.
    pub fn trunc_middle(&self, s: &str, w: usize) -> String {
        let s = collapse_ws(s);
        if s.width() <= w {
            return s;
        }
        let e = self.ellipsis.width();
        if w <= e {
            return head(&s, w);
        }
        let rest = w - e;
        let front = rest.div_ceil(2);
        format!("{}{}{}", head(&s, front), self.ellipsis, tail(&s, rest - front))
    }
}

/// Longest prefix whose display width fits `w`. Measured on the whole prefix,
/// never by summing char widths: `⌨` is 1 and U+FE0F is 0, but the pair is 2.
/// ponytail: O(w²) since width() rescans, and w is a tile title.
fn head(s: &str, w: usize) -> String {
    let mut end = 0;
    for (i, c) in s.char_indices() {
        let next = i + c.len_utf8();
        if s[..next].width() > w {
            break;
        }
        end = next;
    }
    s[..end].to_string()
}

/// Longest suffix whose display width fits `w`.
fn tail(s: &str, w: usize) -> String {
    let mut start = s.len();
    for (i, _) in s.char_indices().rev() {
        if s[i..].width() > w {
            break;
        }
        start = i;
    }
    s[start..].to_string()
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut gap = false;
    for c in s.trim().chars() {
        if c.is_whitespace() {
            gap = true;
        } else {
            if gap {
                out.push(' ');
            }
            gap = false;
            out.push(c);
        }
    }
    out
}

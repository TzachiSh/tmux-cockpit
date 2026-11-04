//! The colour roles and the glyph tier, read once at startup. tmux downgrades
//! colour per attached client, so there is no 16 tier and no terminal query:
//! in a popup tmux answers neither OSC 11 nor DSR 996 (#240).
use crate::data::{State, tmux};
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
    /// the review mark: ◆ must not share "working" yellow with ●
    pub sky: Color,
    /// frames, rules and separators only: never text (@thm_faint, 3:1)
    pub faint: Color,
    /// the selected list row's band (@thm_sel); its text stays `value`
    pub sel: Color,
    pub g: Glyphs,
}

/// field -> (@thm option, dark default, light default): the plugin's palette,
/// shared with the status line (scripts/lib/statusbar.conf). The light set is
/// `<option>_light`. Must-read text is fg or dim, and dim holds 4.5:1 on bg;
/// faint (3:1) is for frames, rules and separators, never for text.
const ROLES: [(&str, &str, &str); 12] = [
    ("@thm_dim", "#9097a3", "#5c626c"),     // label
    ("@thm_fg", "#e6e4df", "#1b1e24"),      // value
    ("@thm_fg", "#e6e4df", "#1b1e24"),      // quote
    ("@thm_dim", "#9097a3", "#5c626c"),     // meta
    ("@thm_accent", "#6cb6ff", "#0a5fc2"),  // accent
    ("@thm_needs", "#ff5f6d", "#c4122f"),   // needs
    ("@thm_error", "#ff9e3d", "#9a4c00"),   // error
    ("@thm_running", "#e6c84f", "#806200"), // running
    ("@thm_done", "#5fd38d", "#1d7439"),    // done
    ("@thm_review", "#5fd3d3", "#00737a"),  // sky
    ("@thm_faint", "#717885", "#7c828c"),   // faint
    ("@thm_sel", "#3a4252", "#c8d1df"),     // sel
];

/// `theme light` and `theme dark` force a set. Under `detect` (or no line) the
/// client's own report decides: tmux 3.8's undocumented `#{client_theme}`, set
/// by a terminal's mode 2031 or DSR 996 report and empty until one comes. Empty
/// is dark, as tmux treats an unknown theme; tmux's other guess, the client's
/// OSC 11 background, has no format. `terminal` keeps the dark set it always had.
fn light_set(opts: &str) -> bool {
    let line = |key: &str| opts.lines().find_map(|l| l.strip_prefix(key));
    match line("theme ") {
        Some("light") => true,
        Some("dark" | "terminal") => false,
        _ => line("client_theme=") == Some("light"),
    }
}

fn hex(s: &str) -> Option<Color> {
    let h = s.trim().trim_matches('"').trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(h, 16).ok()?;
    Some(Color::Rgb((n >> 16) as u8, (n >> 8) as u8, n as u8))
}

impl Theme {
    /// One tmux call, once per process. The light set follows `light_set`.
    pub fn load() -> Self {
        if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
            return Self {
                g: Glyphs::from_env(),
                ..Self::no_color()
            };
        }
        // A bare display-message in a popup resolves the most recently active
        // client (the one whose key or click opened the popup).
        let opts = tmux(&[
            "show-options",
            "-g",
            ";",
            "show-options",
            "-s",
            "theme",
            ";",
            "display-message",
            "-p",
            "client_theme=#{client_theme}",
        ])
        .unwrap_or_default();
        let light = light_set(&opts);
        let mut c = [Color::Reset; 12];
        for (i, (opt, dark, lite)) in ROLES.iter().enumerate() {
            let (name, fallback) = if light {
                (format!("{opt}_light"), lite)
            } else {
                (opt.to_string(), dark)
            };
            let set = opts.lines().find_map(|l| l.strip_prefix(&name)?.strip_prefix(' '));
            c[i] = set.and_then(hex).or_else(|| hex(fallback)).unwrap_or(Color::Reset);
        }
        Self {
            g: Glyphs::from_env(),
            ..Self::from_roles(c)
        }
    }

    fn from_roles(c: [Color; 12]) -> Self {
        Self {
            label: c[0],
            value: c[1],
            quote: c[2],
            meta: c[3],
            accent: c[4],
            needs: c[5],
            error: c[6],
            running: c[7],
            done: c[8],
            sky: c[9],
            faint: c[10],
            sel: c[11],
            g: Glyphs::unicode(),
        }
    }

    /// The test fixture: the shipped defaults with no tmux read, so a snapshot
    /// never moves with the user's palette.
    pub fn fixed() -> Self {
        let mut c = [Color::Reset; 12];
        for (i, (_, fallback, _)) in ROLES.iter().enumerate() {
            c[i] = hex(fallback).unwrap_or(Color::Reset);
        }
        Self::from_roles(c)
    }

    /// NO_COLOR, honoured in-app: crossterm's own support emits an empty SGR that
    /// also drops bold, dim and reverse, which the colourless fallback relies on.
    pub fn no_color() -> Self {
        Self::from_roles([Color::Reset; 12])
    }

    /// Per-state glyph and colour, on an urgency ramp.
    pub fn glyph(&self, state: State) -> (&'static str, Color) {
        match state {
            State::NeedsInput => (self.g.needs, self.needs),
            State::Error => (self.g.error, self.error),
            State::Running => (self.g.running, self.running),
            State::Done => (self.g.done, self.done),
            State::Unknown => (self.g.idle, self.meta),
        }
    }

    pub fn review_mark(&self, review: &str) -> (&'static str, Color) {
        match review {
            "pending" => (self.g.review_pending, self.sky),
            "sent" | "pasted" => (self.g.review_sent, self.meta),
            _ => (" ", self.meta),
        }
    }
}

/// The glyph tier. `ascii` swaps borders too, since `BorderType::Plain` is
/// box-drawing; its ellipsis is three cells, which every budget subtracts.
#[derive(Clone, Copy, PartialEq)]
pub struct Glyphs {
    /// Prints nothing but ASCII, the panel's key names included.
    pub ascii: bool,
    pub needs: &'static str,
    pub error: &'static str,
    pub running: &'static str,
    pub done: &'static str,
    pub idle: &'static str,
    /// #301: an agent whose process is gone; the board's, never a hook state
    pub ended: &'static str,
    pub review_pending: &'static str,
    pub review_sent: &'static str,
    /// #313: the subagent badge on a list row; nerd-font's nf-oct-workflow in the nerd tier
    pub sub: &'static str,
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
    pub star: &'static str,
    pub warn: &'static str,
    pub arrow: &'static str,
    pub ndash: &'static str,
    /// The text box's cursor, one cell wide in both tiers.
    pub cursor: &'static str,
    pub spin: &'static [&'static str],
    pub key_enter: &'static str,
    pub key_tab: &'static str,
    pub key_bksp: &'static str,
    pub key_left: &'static str,
    pub key_right: &'static str,
    pub key_updown: &'static str,
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
        match std::env::var("COCKPIT_GLYPHS").as_deref() {
            Ok("ascii") => Self::ascii(),
            Ok("nerd") => Self::nerd(),
            _ => Self::unicode(),
        }
    }

    pub const fn unicode() -> Self {
        Self {
            ascii: false,
            needs: "▲",
            error: "✕",
            running: "●",
            done: "✓",
            idle: "○",
            ended: "◌",
            review_pending: "◆",
            review_sent: "↵",
            // U+2726, one cell, never followed by U+FE0F
            sub: "\u{2726}",
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
            star: "★",
            warn: "⚠",
            arrow: "→",
            ndash: "–",
            cursor: "▏",
            spin: &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
            key_enter: "↵",
            key_tab: "⇥",
            key_bksp: "⌫",
            key_left: "←",
            key_right: "→",
            key_updown: "↑↓",
        }
    }

    /// The unicode tier with a Nerd Font's glyph where one says more: the subagent
    /// badge only. Every other mark stays the shape that carries the state.
    pub const fn nerd() -> Self {
        Self {
            sub: "\u{f52e}",
            ..Self::unicode()
        }
    }

    pub const fn ascii() -> Self {
        Self {
            ascii: true,
            needs: "!",
            error: "x",
            running: "*",
            done: "+",
            idle: ".",
            ended: "~",
            review_pending: "#",
            review_sent: "<",
            sub: "&",
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
            star: "*",
            warn: "!",
            arrow: "->",
            ndash: "-",
            cursor: "|",
            spin: &["|", "/", "-", "\\"],
            key_enter: "enter",
            key_tab: "tab",
            key_bksp: "bksp",
            key_left: "left",
            key_right: "right",
            key_updown: "up/dn",
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

/// Longest prefix whose display width fits `w`, measured on the whole prefix:
/// `⌨` is 1 and U+FE0F is 0, but the pair is 2. O(w²), fine for a title.
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

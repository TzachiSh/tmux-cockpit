//! The colour roles of screens.md §0, read once at startup from the server's
//! @thm_* options, and always emitted as RGB: measured on 3.7c, tmux does the
//! downgrade itself per attached client (nearest-256 without RGB, 16 on a
//! 16-colour client), so a `#{client_termfeatures}` gate only picks one client
//! of several and throws the user's palette away on a client-less server.
use crate::data::tmux;
use ratatui::style::{Color, Modifier, Style};
use std::collections::HashMap;

#[derive(Clone, Copy)]
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
    pub sel_bg: Color,
}

/// role -> (@thm option, Catppuccin Mocha default, the fixed 256 index tests pin)
const ROLES: [(&str, &str, u8); 9] = [
    ("@thm_overlay_1", "#7f849c", 245),
    ("@thm_subtext_0", "#a6adc8", 250),
    ("@thm_overlay_0", "#6c7086", 243),
    ("@thm_lavender", "#b4befe", 147),
    ("@thm_red", "#f38ba8", 210),
    ("@thm_peach", "#fab387", 216),
    ("@thm_yellow", "#f9e2af", 223),
    ("@thm_green", "#a6e3a1", 151),
    ("@thm_surface_0", "#313244", 236),
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
        let opts = tmux(&["show-options", "-g"]).unwrap_or_default();
        let mut set: HashMap<&str, &str> = HashMap::new();
        for l in opts.lines() {
            if let Some((k, v)) = l.split_once(' ')
                && k.starts_with("@thm_")
            {
                set.insert(k, v);
            }
        }
        // Every role stays Reset under NO_COLOR. Ours and not crossterm's: its
        // NO_COLOR emits an empty SGR that also drops bold, dim and reverse, the
        // three attributes a colourless fallback is built on.
        let mut c = [Color::Reset; 9];
        if std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()) {
            for (i, (opt, fallback, _)) in ROLES.iter().enumerate() {
                c[i] = set.get(opt).and_then(|v| hex(v)).or_else(|| hex(fallback)).unwrap_or(Color::Reset);
            }
        }
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
            sel_bg: c[8],
        }
    }

    /// The test fixture: fixed 256-colour indices, so a snapshot never moves
    /// with the user's palette.
    pub fn fixed() -> Self {
        let mut t = Self::load_defaults();
        t.value = Color::Reset;
        t
    }

    fn load_defaults() -> Self {
        Self {
            label: Color::Indexed(245),
            value: Color::Reset,
            quote: Color::Indexed(250),
            meta: Color::Indexed(243),
            accent: Color::Indexed(147),
            needs: Color::Indexed(210),
            error: Color::Indexed(216),
            running: Color::Indexed(223),
            done: Color::Indexed(151),
            sel_bg: Color::Indexed(236),
        }
    }

    /// The glyph tier of screens.md §0. Only unicode and ascii ship; nerd would
    /// be a third table and nothing in the default path needs one.
    pub fn glyph(&self, state: &str) -> (&'static str, Color) {
        let ascii = std::env::var("COCKPIT_GLYPHS").as_deref() == Ok("ascii");
        match state {
            "needs-input" => (if ascii { "!" } else { "▲" }, self.needs),
            "error" => (if ascii { "x" } else { "✕" }, self.error),
            "running" => (if ascii { "*" } else { "●" }, self.running),
            "done" => (if ascii { "+" } else { "✓" }, self.done),
            _ => (if ascii { "." } else { "○" }, self.meta),
        }
    }

    pub fn review_mark(&self, review: &str) -> (&'static str, Color) {
        let ascii = std::env::var("COCKPIT_GLYPHS").as_deref() == Ok("ascii");
        match review {
            "pending" => (if ascii { "#" } else { "◆" }, self.accent),
            "sent" | "pasted" => (if ascii { "<" } else { "↵" }, self.meta),
            _ => (" ", self.meta),
        }
    }

    pub fn sel(&self) -> Style {
        Style::new().bg(self.sel_bg).add_modifier(Modifier::BOLD)
    }
}

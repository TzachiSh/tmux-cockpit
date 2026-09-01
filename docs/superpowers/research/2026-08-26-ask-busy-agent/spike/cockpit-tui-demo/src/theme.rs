//! The colour roles of screens.md §0, read once at startup from the server's
//! @thm_* options. Truecolor or 256 is decided from #{client_termfeatures},
//! never by querying the terminal (a query inside display-popup can hang).
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

/// role -> (@thm option, Catppuccin Mocha default, 256 index)
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
    /// Two tmux calls, once per process.
    pub fn load() -> Self {
        let rgb = tmux(&["display-message", "-p", "#{client_termfeatures}"]).contains("RGB");
        let opts = tmux(&["show-options", "-g"]);
        let mut set: HashMap<&str, &str> = HashMap::new();
        for l in opts.lines() {
            if let Some((k, v)) = l.split_once(' ')
                && k.starts_with("@thm_")
            {
                set.insert(k, v);
            }
        }
        let mut c = [Color::Reset; 9];
        for (i, (opt, fallback, idx)) in ROLES.iter().enumerate() {
            c[i] = if rgb {
                set.get(opt).and_then(|v| hex(v)).or_else(|| hex(fallback)).unwrap_or(Color::Reset)
            } else {
                Color::Indexed(*idx)
            };
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

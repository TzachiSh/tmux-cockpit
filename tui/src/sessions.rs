//! `cockpit-tui sessions`: the tmux session picker (#245). Rows are sessions;
//! the screen is pick.rs's, with its own row, footer and help. Like the board it
//! never mutates: it prints one action line and the caller (scripts/tm) acts.
//!
//! The action line is `<verb> <target>`, the target a tmux id (`$N`) or `-`. What
//! is free-form travels beside it: `$COCKPIT_ACTION.text` carries a new name,
//! `$COCKPIT_ACTION.expect` the session name the person was shown, for the caller
//! to check against the live name before it kills or renames. Only `switch`
//! needs no side file, so only `switch` works with `COCKPIT_ACTION` unset.
use crate::data::sessions::{self, Sess};
use crate::data::{State, fmt_dur};
use crate::panel::view::wrap;
use crate::pick::{self, Look, Pick};
use crate::theme::{Glyphs, Theme};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::mpsc;
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

const USAGE: &str = "cockpit-tui sessions [--needs] [--current $N]";
const REFRESH: Duration = Duration::from_secs(2);
/// Columns before the name: marker, digit, state glyph and their spaces.
const LEAD: usize = 6;

pub struct Args {
    pub needs: bool,
    pub current: Option<String>,
}

pub fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut a = Args {
        needs: false,
        current: None,
    };
    let mut it = args.iter();
    while let Some(x) = it.next() {
        match x.as_str() {
            "--needs" => a.needs = true,
            "--current" => {
                let v = it.next().ok_or("--current needs a session id like $3")?;
                if !sessions::is_sid(v) {
                    return Err(format!("--current wants a session id like $3, not {v}"));
                }
                a.current = Some(v.clone());
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(a)
}

/// What the person chose. `line` is the action line without its newline.
#[derive(Debug, PartialEq, Eq)]
pub struct Act {
    pub line: String,
    pub text: Option<String>,
    pub expect: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Continue,
    Act(Act),
    Cancel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Mode {
    Normal,
    /// ^X was pressed once on this session; the header names what dies.
    Armed {
        id: String,
        name: String,
    },
    Rename {
        id: String,
        old: String,
        buf: String,
    },
}

pub struct View {
    pub pick: Pick,
    pub sess: Vec<Sess>,
    cur: Option<String>,
    needs: bool,
    /// `--needs` found a session waiting or failed, so the view is only those.
    only: bool,
    mode: Mode,
    pub note: Option<String>,
    carry: bool,
    now: u64,
    g: Glyphs,
}

fn rows(v: &[Sess]) -> Vec<pick::Row> {
    v.iter()
        .map(|s| pick::Row {
            key: s.id.clone().into_bytes(),
            text: pick::clean(&s.name),
        })
        .collect()
}

fn plural(n: usize, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}

impl View {
    /// `carry`: a place for the text and expect files, i.e. `COCKPIT_ACTION` is set.
    pub fn new(fresh: Vec<Sess>, a: &Args, carry: bool, now: u64, g: Glyphs) -> Self {
        let (mut v, only) = if a.needs { sessions::needs(fresh) } else { (fresh, false) };
        sessions::order(&mut v, a.current.as_deref(), now);
        let mut pick = Pick::new(
            rows(&v),
            &pick::Opts {
                prompt: "session".into(),
                header: String::new(),
                preview: None,
            },
            Some(String::new()),
        );
        pick.list_pct = 55;
        Self {
            pick,
            sess: v,
            cur: a.current.clone(),
            needs: a.needs,
            only,
            mode: Mode::Normal,
            note: None,
            carry,
            now,
            g,
        }
    }

    /// A refresh: states and ages move, the order does not. A session that went
    /// takes an armed kill or a rename with it.
    pub fn refresh(&mut self, fresh: Vec<Sess>, now: u64, area: Rect) {
        let ids: Vec<String> = self.sess.iter().map(|s| s.id.clone()).collect();
        self.sess = sessions::freeze(&ids, fresh, self.needs && self.only);
        self.now = now;
        self.pick.set_rows(rows(&self.sess), area);
        let gone = match &self.mode {
            Mode::Armed { id, .. } | Mode::Rename { id, .. } => !self.sess.iter().any(|s| s.id == *id),
            Mode::Normal => false,
        };
        if gone {
            self.leave();
            self.note = Some(format!("{} that session is gone", self.g.error));
        }
    }

    fn leave(&mut self) {
        self.mode = Mode::Normal;
        self.pick.entry = None;
        self.pick.prompt = "session".into();
    }

    pub fn selected(&self) -> Option<&Sess> {
        let &i = self.pick.shown.get(self.pick.sel)?;
        self.sess.iter().find(|s| s.id.as_bytes() == self.pick.rows[i].key)
    }

    fn mine(&self, s: &Sess) -> bool {
        self.cur.as_deref() == Some(s.id.as_str())
    }

    fn refuse(&mut self, why: &str) -> Step {
        self.note = Some(format!("{} {why}", self.g.error));
        Step::Continue
    }

    fn switch(s: &Sess) -> Step {
        Step::Act(Act {
            line: format!("switch {}", s.id),
            text: None,
            expect: None,
        })
    }

    fn create(&mut self) -> Step {
        let name = sessions::clean_name(&self.pick.query);
        if name.is_empty() {
            return self.refuse("type a name first");
        }
        if !self.carry {
            return self.refuse("creating needs COCKPIT_ACTION");
        }
        Step::Act(Act {
            line: "new-session -".into(),
            text: Some(name),
            expect: None,
        })
    }

    fn arm(&mut self) -> Step {
        let Some(s) = self.selected() else { return Step::Continue };
        let (id, name, mine) = (s.id.clone(), pick::clean(&s.name), self.mine(s));
        if mine {
            return self.refuse(&format!("{name} is your own session: detach, or kill it from another"));
        }
        if !self.carry {
            return self.refuse("killing needs COCKPIT_ACTION");
        }
        self.mode = Mode::Armed { id, name };
        Step::Continue
    }

    fn rename(&mut self) -> Step {
        let Some(s) = self.selected() else { return Step::Continue };
        let (id, old) = (s.id.clone(), pick::clean(&s.name));
        if !self.carry {
            return self.refuse("renaming needs COCKPIT_ACTION");
        }
        self.pick.entry = Some(old.clone());
        self.pick.prompt = "rename".into();
        self.mode = Mode::Rename {
            id,
            old: old.clone(),
            buf: old,
        };
        Step::Continue
    }

    fn rename_key(&mut self, k: KeyEvent) -> Step {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let Mode::Rename { id, old, buf } = &mut self.mode else {
            return Step::Continue;
        };
        match k.code {
            KeyCode::Esc => {
                self.leave();
                return Step::Continue;
            }
            KeyCode::Enter => {
                let (id, old, new) = (id.clone(), old.clone(), sessions::clean_name(buf));
                self.leave();
                if new.is_empty() || new == old {
                    return self.refuse("the name is empty or unchanged");
                }
                return Step::Act(Act {
                    line: format!("rename-session {id}"),
                    text: Some(new),
                    expect: Some(old),
                });
            }
            KeyCode::Backspace => {
                buf.pop();
            }
            KeyCode::Char('u') if ctrl => buf.clear(),
            KeyCode::Char(c) if k.modifiers.difference(KeyModifiers::SHIFT).is_empty() => buf.push(c),
            _ => {}
        }
        self.pick.entry = Some(buf.clone());
        Step::Continue
    }

    pub fn key(&mut self, k: KeyEvent, area: Rect) -> Step {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        if ctrl && k.code == KeyCode::Char('c') {
            return Step::Cancel;
        }
        if pick::too_small(area) || self.pick.help.is_some() {
            return match self.pick.key(k, area) {
                pick::Outcome::Cancel => Step::Cancel,
                _ => Step::Continue,
            };
        }
        self.note = None;
        match self.mode.clone() {
            // any key but the second ^X puts the session back; the key is spent
            Mode::Armed { id, name } => {
                self.mode = Mode::Normal;
                if ctrl && k.code == KeyCode::Char('x') {
                    return Step::Act(Act {
                        line: format!("kill-session {id}"),
                        text: None,
                        expect: Some(name),
                    });
                }
                return Step::Continue;
            }
            Mode::Rename { .. } => return self.rename_key(k),
            Mode::Normal => {}
        }
        match k.code {
            KeyCode::Enter if alt => return self.create(),
            KeyCode::Enter => {
                return match self.selected() {
                    Some(s) => Self::switch(s),
                    None if self.pick.query.is_empty() => Step::Continue,
                    None => self.create(),
                };
            }
            KeyCode::Char('x') if ctrl => return self.arm(),
            KeyCode::F(2) => return self.rename(),
            KeyCode::Char(c @ '1'..='9') if self.pick.query.is_empty() && k.modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                // the digit shown is the frozen position, and the list is whole while the filter is empty
                return match self.sess.get(c as usize - '1' as usize) {
                    Some(s) => Self::switch(s),
                    None => Step::Continue,
                };
            }
            _ => {}
        }
        match self.pick.key(k, area) {
            pick::Outcome::Cancel => Step::Cancel,
            _ => Step::Continue,
        }
    }

    /// A click selects; a click on the row already selected chooses it, so a
    /// touch screen needs two taps and a stray one never switches.
    pub fn mouse(&mut self, m: MouseEvent, area: Rect) -> Step {
        if let Mode::Armed { .. } = self.mode
            && matches!(m.kind, MouseEventKind::Down(_))
        {
            self.mode = Mode::Normal;
            return Step::Continue;
        }
        if self.mode != Mode::Normal || pick::too_small(area) || self.pick.help.is_some() {
            return Step::Continue;
        }
        // the header's row decides where the list starts
        self.pick.header = self.header();
        let before = self.pick.sel;
        self.pick.mouse(m, area);
        let list = pick::geo(area, &self.pick).list;
        let hit = matches!(m.kind, MouseEventKind::Down(MouseButton::Left))
            && list.contains((m.column, m.row).into())
            && self.pick.top + (m.row - list.y) as usize == before
            && before < self.pick.shown.len();
        match self.selected() {
            Some(s) if hit => Self::switch(s),
            _ => Step::Continue,
        }
    }

    fn glyph_counts(&self) -> String {
        let mut out = String::new();
        for (s, glyph) in [
            (State::NeedsInput, self.g.needs),
            (State::Error, self.g.error),
            (State::Running, self.g.running),
        ] {
            let n = self.sess.iter().filter(|x| x.state() == Some(s)).count();
            if n > 0 {
                out.push_str(&format!(" {glyph}{n}"));
            }
        }
        out
    }

    /// The line under the prompt: a note, else the armed kill, else the tally.
    pub fn header(&self) -> String {
        if let Some(n) = &self.note {
            return n.clone();
        }
        if let Mode::Armed { id, name } = &self.mode {
            let what = self.sess.iter().find(|s| s.id == *id).map(|s| self.doomed(s)).unwrap_or_default();
            return format!("kill {name}? {what}");
        }
        let n = plural(self.sess.len(), "session");
        let dot = self.g.dot;
        let tally = self.glyph_counts();
        match (self.needs, self.only) {
            (true, true) => format!("needs you {dot}{tally}"),
            (true, false) => format!(
                "nothing needs you {dot} {n}{}",
                if tally.is_empty() {
                    String::new()
                } else {
                    format!(" {dot}{tally}")
                }
            ),
            _ if tally.is_empty() => n,
            _ => format!("{n} {dot}{tally}"),
        }
    }

    /// What a kill takes with it, the most surprising first.
    fn doomed(&self, s: &Sess) -> String {
        let mut v = vec![];
        if s.attached > 0 {
            v.push("attached elsewhere".to_string());
        }
        v.push(match (s.agents(), s.waiting()) {
            (0, _) => "no agents".to_string(),
            (n, 0) => plural(n, "agent"),
            (n, w) => format!("{} ({w} waiting)", plural(n, "agent")),
        });
        v.push(plural(s.wins.len(), "window"));
        v.join(&format!(" {} ", self.g.dot))
    }

    /// The windows with their state, and the question a waiting one asks.
    pub fn preview(&self, th: &Theme, width: usize) -> String {
        let Some(s) = self.selected() else { return String::new() };
        let mut out = vec![format!("{} {} {}", pick::clean(&s.name), th.g.dot, plural(s.wins.len(), "window"))];
        for w in &s.wins {
            let (glyph, word) = if w.agents == 0 {
                (" ", "no agent".to_string())
            } else {
                let word = match w.state {
                    State::NeedsInput => "needs you",
                    State::Error => "error",
                    State::Running => "running",
                    State::Done => "done",
                    State::Unknown => "idle",
                };
                (th.glyph(w.state).0, word.to_string())
            };
            let here = if w.active { th.g.active } else { " " };
            out.push(format!("{here}{} {glyph} {} {}", w.idx, pick::clean(&w.name), word));
            for l in wrap(&pick::clean(&w.ask), width.saturating_sub(6).max(8)).into_iter().take(3) {
                out.push(format!("      {l}"));
            }
        }
        out.join("\n")
    }

    /// The footer's hints for the mode.
    fn hints(&self) -> Vec<(String, String)> {
        let h = |k: &str, v: &str| (k.to_string(), v.to_string());
        let e = self.g.key_enter;
        match &self.mode {
            Mode::Armed { .. } => vec![h("^X", "kill it"), h("esc", "keep it")],
            Mode::Rename { .. } => vec![h(e, "rename"), h("esc", "cancel")],
            Mode::Normal if self.pick.shown.is_empty() && !self.pick.query.is_empty() => {
                vec![h(e, "create"), h("F1", "keys"), h("esc", "cancel")]
            }
            Mode::Normal => {
                let mut v = vec![h(e, "switch")];
                if self.carry {
                    v.push(h("^X", "kill"));
                    v.push(h("F2", "rename"));
                }
                v.push(h("F1", "keys"));
                v.push(h("esc", "cancel"));
                v
            }
        }
    }

    pub fn help(&self) -> Vec<(String, String)> {
        let r = |k: &str, v: &str| (k.to_string(), v.to_string());
        let g = &self.g;
        vec![
            r("", "type"),
            r("any key", "adds to the filter, j k q ? included"),
            r(&format!("{}  ^U", g.key_bksp), "delete a letter / clear the filter"),
            r("", "move"),
            r(&format!("{}  ^N ^P", g.key_updown), "one row"),
            r("1-9", "switch to that row while the filter is empty"),
            r("click", "select a row; click it again to switch"),
            r("", "act"),
            r(g.key_enter, "switch to the row; with no match, create the typed name"),
            r("alt-enter", "create the typed name even when a row matches"),
            r("^X", "kill the row; ^X again confirms, any other key keeps it"),
            r("F2", "rename the row; enter confirms, esc cancels"),
            r("", "leave"),
            r("esc  ^C", "cancel, print nothing"),
            r("F1", "this help"),
        ]
    }

    fn line(&self, r: &pick::Row, sel: bool, w: usize, th: &Theme) -> Line<'static> {
        let i = self.sess.iter().position(|s| s.id.as_bytes() == r.key).unwrap_or(0);
        let s = &self.sess[i];
        let here = self.mine(s);
        let (value, meta, accent) = (Style::new().fg(th.value), Style::new().fg(th.meta), Style::new().fg(th.accent));
        let (glyph, gc) = match s.state() {
            Some(st) => th.glyph(st),
            None => (" ", th.meta),
        };
        let digit = if i < 9 { (i + 1).to_string() } else { " ".into() };
        let others = s.attached.saturating_sub(usize::from(here));
        let att = if others > 0 { if th.g.ascii { "@" } else { "◉" } } else { " " };
        let age = if here { String::new() } else { fmt_dur(s.age(self.now)) };
        let (na, nw) = (s.agents(), s.wins.len());
        // wide first; the name needs 16 cells to afford the long counts, 10 the short
        let right = if w >= LEAD + 16 + 25 {
            let ag = if na == 0 { th.g.dash.to_string() } else { plural(na, "agent") };
            format!("{ag:<9} {:<6} {att} {age:>5}", plural(nw, "win"))
        } else if w >= LEAD + 10 + 16 {
            let ag = if na == 0 { "-".to_string() } else { format!("{na}a") };
            format!("{:<7} {att} {age:>5}", format!("{ag} {nw}w"))
        } else {
            format!("{att} {age:>5}")
        };
        let room = w.saturating_sub(LEAD + right.width() + 1);
        let tag = if here { " (here)" } else { "" };
        let name = th.g.trunc(&pick::clean(&s.name), room.saturating_sub(tag.len()).max(1));
        let used = LEAD + name.width() + tag.len();
        let pad = w.saturating_sub(used + right.width());
        let name_style = if sel { value.add_modifier(Modifier::BOLD) } else { value };
        Line::from(vec![
            if sel { Span::styled(th.g.active, accent) } else { Span::raw(" ") },
            Span::styled(format!(" {digit} "), meta),
            Span::styled(glyph, Style::new().fg(gc)),
            Span::raw(" "),
            Span::styled(name, name_style),
            Span::styled(tag, meta),
            Span::raw(" ".repeat(pad)),
            Span::styled(right, meta),
        ])
    }
}

/// Syncs what depends on the selection and the mode, then draws.
pub fn draw(f: &mut Frame, area: Rect, v: &mut View, th: &Theme) {
    v.pick.header = v.header();
    let pw = pick::geo(area, &v.pick).preview.map_or(0, |r| r.width as usize);
    v.pick.preview = Some(v.preview(th, pw));
    let row = |r: &pick::Row, sel: bool, w: usize| v.line(r, sel, w, th);
    let look = Look {
        title: Some("sessions"),
        hints: Some(v.hints()),
        help: Some(v.help()),
        row: Some(&row),
        empty: Some(if v.pick.query.is_empty() {
            "no sessions".into()
        } else if v.carry {
            format!("no match for \"{}\" {} {} creates it", v.pick.query, th.g.dot, th.g.key_enter)
        } else {
            format!("no match for \"{}\"", v.pick.query)
        }),
    };
    pick::render_with(f, area, &v.pick, th, &look);
}

/// Created fresh and never followed, mode 0600: a planted file or link in
/// the action's directory must not take the write.
fn fresh(p: &str, body: &str) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .mode(0o600)
        .open(p)?;
    f.write_all(body.as_bytes())
}

/// The side files first, the action line last, so the caller never reads a
/// line whose text is not there yet. Without `COCKPIT_ACTION` only `switch`
/// has anywhere to go: stdout.
pub fn emit(a: &Act, file: Option<&str>) -> std::io::Result<()> {
    let Some(f) = file else {
        println!("{}", a.line);
        return Ok(());
    };
    if let Some(t) = &a.text {
        fresh(&format!("{f}.text"), &format!("{t}\n"))?;
    }
    if let Some(e) = &a.expect {
        fresh(&format!("{f}.expect"), &format!("{e}\n"))?;
    }
    fresh(f, &format!("{}\n", a.line))
}

/// Exit 0 with an action written, 1 for a cancel (nothing written), 2 for a bad
/// argument, no terminal or no tmux, 3 when the action could not be written.
pub fn main(args: &[String]) -> i32 {
    let a = match parse_args(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("cockpit-tui: sessions: {e}\n{USAGE}");
            return 2;
        }
    };
    let file = std::env::var("COCKPIT_ACTION").ok().filter(|f| !f.is_empty());
    let Some(first) = sessions::fetch() else {
        eprintln!("cockpit-tui: sessions: tmux is not reachable");
        return 2;
    };
    let th = Theme::load();
    let mut v = View::new(first, &a, file.is_some(), sessions::now(), th.g);
    let mut term = match pick::open_screen("sessions") {
        Ok(t) => t,
        Err(c) => return c,
    };
    // The fetch forks tmux, so it runs here and never on the key thread.
    let (tx, rx) = mpsc::channel();
    let (stop, parked) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        while let Err(mpsc::RecvTimeoutError::Timeout) = parked.recv_timeout(REFRESH) {
            if let Some(s) = sessions::fetch()
                && tx.send(s).is_err()
            {
                return;
            }
        }
    });
    let mut dirty = true;
    let step = loop {
        let area = pick::screen(&term);
        v.pick.clamp(area);
        if dirty && term.draw(|f| draw(f, f.area(), &mut v, &th)).is_err() {
            break Step::Cancel;
        }
        dirty = false;
        if let Some(s) = rx.try_iter().last() {
            v.refresh(s, sessions::now(), area);
            dirty = true;
        }
        match event::poll(Duration::from_millis(200)) {
            Ok(false) => continue,
            Err(_) => break Step::Cancel,
            Ok(true) => {}
        }
        dirty = true;
        let s = match event::read() {
            Ok(Event::Key(k)) if k.kind != KeyEventKind::Release => v.key(k, area),
            Ok(Event::Mouse(m)) => v.mouse(m, area),
            Ok(_) => Step::Continue,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => Step::Continue,
            Err(_) => Step::Cancel,
        };
        if s != Step::Continue {
            break s;
        }
    };
    drop(stop);
    drop(term);
    pick::restore_tty();
    match step {
        Step::Act(act) => match emit(&act, file.as_deref()) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("cockpit-tui: sessions: cannot write the action: {e}");
                3
            }
        },
        _ => 1,
    }
}

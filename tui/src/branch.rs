//! `cockpit-tui branch`: the branch popup (#246). It lists the repo's branches in
//! the shared picker, previews the selection's graph, and prints one action
//! line. It never mutates; `scripts/tmux-branch` validates the line and acts.
//!
//! The line is `switch <branch>`, `track <remote>/<branch>` or `create <name>`.
//! A name is validated here with git's own rule (the same one the script runs
//! again), so a bad name is a note on this screen and not a failed action.
use crate::chrome;
use crate::data::branches::{self, Branch};
use crate::pick::{self, Opts, Outcome, Pick, Row};
use crate::theme::{Glyphs, Theme};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent};
use ratatui::layout::Rect;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender, channel};
use unicode_width::UnicodeWidthStr;

/// A key that is no action: the branch is held by another worktree.
const BUSY: &str = "busy ";
const NAME_COL: usize = 20;

pub struct Ui {
    pub pick: Pick,
    branches: Vec<Branch>,
    dir: String,
    header: String,
    ascii: bool,
    cache: HashMap<String, String>,
    asked: HashSet<String>,
    tx: Option<Sender<String>>,
}

fn pad(s: &str, w: usize) -> String {
    let n = s.width();
    format!("{s}{}", " ".repeat(w.saturating_sub(n)))
}

/// The row's key is the action line itself, exact bytes; its text is for the eye.
fn row(b: &Branch, g: &Glyphs) -> Row {
    let (cur, wt) = if g.ascii { ("*", "w") } else { ("●", "⑂") };
    let key = match (&b.elsewhere, b.remote) {
        (Some(_), _) => format!("{BUSY}{}", b.name),
        (None, true) => format!("track {}", b.name),
        (None, false) => format!("switch {}", b.name),
    };
    let (m1, m2) = (if b.current { cur } else { " " }, if b.elsewhere.is_some() { wt } else { " " });
    let held = b.elsewhere.as_deref().map(|d| format!(" {}", pick::clean(d))).unwrap_or_default();
    let text = format!("{m1}{m2} {} ({}){held}", pad(&pick::clean(&b.name), NAME_COL), pick::clean(&b.age));
    Row {
        key: key.into_bytes(),
        text,
    }
}

impl Ui {
    pub fn new(dir: &str, bs: Vec<Branch>, header: &str, g: &Glyphs, tx: Option<Sender<String>>) -> Self {
        let o = Opts {
            prompt: "branch".into(),
            header: header.into(),
            preview: None,
        };
        let mut u = Self {
            pick: Pick::new(bs.iter().map(|b| row(b, g)).collect(), &o, None),
            branches: bs,
            dir: dir.into(),
            header: pick::clean(header),
            ascii: g.ascii,
            cache: HashMap::new(),
            asked: HashSet::new(),
            tx,
        };
        u.pick.help_len = help(g).len();
        u.sync();
        u
    }

    fn note(&mut self, t: &str) {
        let x = if self.ascii { "x" } else { "✕" };
        self.pick.header = format!("{x} {}", pick::clean(t));
    }

    fn selected(&self) -> Option<&Branch> {
        self.branches.get(*self.pick.shown.get(self.pick.sel)?)
    }

    /// Shows the cached graph of the selection, or asks the worker for it.
    fn sync(&mut self) {
        let Some(b) = self.selected() else {
            self.pick.preview = None;
            return;
        };
        let name = b.name.clone();
        if let Some(t) = self.cache.get(&name) {
            self.pick.preview = Some(t.clone());
            return;
        }
        self.pick.preview = Some(format!("{}\n", if self.ascii { "loading..." } else { "loading…" }));
        if self.asked.insert(name.clone())
            && let Some(tx) = &self.tx
        {
            let _ = tx.send(name);
        }
    }

    /// A graph arrived from the worker.
    pub fn feed(&mut self, branch: String, text: &str) {
        let t = text.lines().map(pick::clean).collect::<Vec<_>>().join("\n");
        self.cache.insert(branch, t);
        self.sync();
    }

    /// Nothing is chosen by a key that is no action, and a note is cleared by the next key.
    pub fn key(&mut self, k: KeyEvent, area: Rect) -> Outcome {
        self.pick.header = self.header.clone();
        let out = if k.code == KeyCode::Enter && k.modifiers.contains(KeyModifiers::ALT) {
            self.create()
        } else {
            match self.pick.key(k, area) {
                Outcome::Choose(key) if key.starts_with(BUSY.as_bytes()) => {
                    let name = String::from_utf8_lossy(&key[BUSY.len()..]).into_owned();
                    let d = self.selected().and_then(|b| b.elsewhere.clone()).unwrap_or_default();
                    self.note(&format!("{name} is checked out in the worktree {d}"));
                    Outcome::Continue
                }
                o => o,
            }
        };
        self.sync();
        out
    }

    fn create(&mut self) -> Outcome {
        let q = self.pick.query.clone();
        if q.is_empty() {
            self.note("type a name before alt-enter");
            Outcome::Continue
        } else if !branches::valid_name(&self.dir, &q) {
            self.note(&format!("not a valid branch name: {q}"));
            Outcome::Continue
        } else {
            Outcome::Choose(format!("create {q}").into_bytes())
        }
    }

    pub fn mouse(&mut self, m: MouseEvent, area: Rect) {
        self.pick.mouse(m, area);
        self.sync();
    }
}

/// Runs git log for each branch asked of it, off the UI thread.
pub fn worker(dir: String, all: Vec<Branch>) -> (Sender<String>, Receiver<(String, String)>) {
    let (tx, rx) = channel::<String>();
    let (otx, orx) = channel();
    std::thread::spawn(move || {
        while let Ok(name) = rx.recv() {
            let Some(b) = all.iter().find(|b| b.name == name) else { continue };
            if otx.send((name, branches::graph(&dir, b))).is_err() {
                break;
            }
        }
    });
    (tx, orx)
}

const USAGE: &str = "cockpit-tui branch [-C <dir>]";

pub fn parse_args(args: &[String]) -> Result<String, String> {
    match args {
        [] => Ok(".".into()),
        [f, d] if f == "-C" && !d.is_empty() => Ok(d.clone()),
        [f] if f == "-C" => Err("-C needs a directory".into()),
        [a, ..] => Err(format!("unknown argument {a}")),
    }
}

/// The footer: pick's keys plus the one this screen adds. ↵ and esc stay when it narrows.
pub fn hints(g: &Glyphs, w: usize) -> Vec<(String, String)> {
    let h = |k: &str, v: &str| (k.to_string(), v.to_string());
    let alt = if g.ascii { "alt-enter" } else { "alt-↵" };
    let fitted = chrome::fit(
        vec![
            h(g.key_enter, "switch"),
            h(alt, "new"),
            h(g.key_updown, "move"),
            h("F1", "keys"),
            h("esc", "cancel"),
        ],
        w,
        3,
    );
    if fitted.iter().any(|(k, _)| k == g.key_enter) {
        fitted
    } else {
        vec![h(g.key_enter, "switch"), h("esc", "cancel")]
    }
}

/// pick's F1 help with the one key this screen adds, under "choose".
pub fn help(g: &Glyphs) -> Vec<(String, String)> {
    let mut rows = pick::help_rows(g);
    let alt = if g.ascii { "alt-enter" } else { "alt-↵" };
    let at = rows.iter().position(|(k, _)| k == g.key_enter).map_or(rows.len(), |i| i + 1);
    rows.insert(at, (alt.to_string(), "print create <filter text>, exit".to_string()));
    rows
}

pub fn draw(f: &mut ratatui::Frame, area: Rect, u: &Ui, th: &Theme) {
    let look = pick::Look {
        title: Some("branch"),
        hints: Some(hints(&th.g, area.width as usize)),
        help: Some(help(&th.g)),
        ..Default::default()
    };
    pick::render_with(f, area, &u.pick, th, &look);
}

fn header(dir: &str, bs: &[Branch], g: &Glyphs) -> String {
    let cur = bs.iter().find(|b| b.current).map_or("detached HEAD", |b| b.name.as_str());
    let leaf = std::fs::canonicalize(dir)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
    format!("on {cur} {} {}", g.dot, leaf.unwrap_or_default())
}

/// Exit 0 with the action line on stdout, 1 for esc, 2 for a bad argument, no
/// repository or no terminal.
pub fn main(args: &[String]) -> i32 {
    let dir = match parse_args(args) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("cockpit-tui: branch: {e}\n{USAGE}");
            return 2;
        }
    };
    if !branches::is_repo(&dir) {
        eprintln!("cockpit-tui: branch: not a git repository: {dir}");
        return 2;
    }
    let mut term = match pick::open_screen("branch") {
        Ok(t) => t,
        Err(rc) => return rc,
    };
    let th = Theme::load();
    let bs = branches::list(&dir);
    let head = header(&dir, &bs, &th.g);
    let (tx, rx) = worker(dir.clone(), bs.clone());
    let mut ui = Ui::new(&dir, bs, &head, &th.g, Some(tx));
    let mut dirty = true;
    let out = loop {
        while let Ok((b, t)) = rx.try_recv() {
            ui.feed(b, &t);
            dirty = true;
        }
        if dirty {
            ui.pick.clamp(pick::screen(&term));
            if term.draw(|f| draw(f, f.area(), &ui, &th)).is_err() {
                break Outcome::Cancel;
            }
            dirty = false;
        }
        // short, so a graph that arrives while no key is pressed still shows
        match event::poll(std::time::Duration::from_millis(50)) {
            Ok(false) => continue,
            Ok(true) => {}
            Err(_) => break Outcome::Cancel,
        }
        dirty = true;
        match event::read() {
            Ok(Event::Key(k)) if k.kind != KeyEventKind::Release => match ui.key(k, pick::screen(&term)) {
                Outcome::Continue => {}
                o => break o,
            },
            Ok(Event::Mouse(m)) => ui.mouse(m, pick::screen(&term)),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break Outcome::Cancel,
        }
    };
    drop(term);
    pick::restore_tty();
    pick::answer(&mut std::io::stdout().lock(), &out)
}

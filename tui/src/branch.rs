//! `cockpit-tui branch`: the branch popup (#246). It lists the repo's branches in
//! the shared picker and previews the selection's graph. A chosen row is an action
//! line: `switch <branch>`, `track <remote>/<branch>`, `create <name>` or, for a
//! branch another worktree holds, `goto <worktree path>` (#303).
//!
//! The popup runs every action itself ([`choose`], #228): switch, track and create
//! through git, and goto through tmux, to the pane working in that tree or a new window
//! there. A refusal is a note on this screen and the picker stays; nothing is printed.
//! A name is validated with git's own rule before anything runs.
use crate::chrome;
use crate::data;
use crate::data::branches::{self, Branch};
use crate::data::gitsafe;
use crate::pick::{self, Opts, Outcome, Pick, Row};
use crate::theme::{Glyphs, Theme};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent};
use ratatui::layout::Rect;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender, channel};
use unicode_width::UnicodeWidthStr;

/// A key that is no action: the branch is held by another worktree whose path
/// cannot ride on one line.
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
    /// A failed action's message holds the preview: a graph arriving or a mouse event
    /// leaves it, and only the next key clears it.
    failed: bool,
}

fn pad(s: &str, w: usize) -> String {
    let n = s.width();
    format!("{s}{}", " ".repeat(w.saturating_sub(n)))
}

/// The directory name of a worktree path, for the eye.
fn leaf(d: &str) -> &str {
    d.rsplit('/').next().unwrap_or(d)
}

/// The row's key is the action line itself, exact bytes; its text is for the eye.
fn row(b: &Branch, g: &Glyphs) -> Row {
    let (cur, wt) = if g.ascii { ("*", "w") } else { ("●", "⑂") };
    let key = match (&b.elsewhere, b.remote) {
        (Some(d), _) if d.starts_with('/') && !d.contains('\n') => format!("goto {d}"),
        (Some(_), _) => format!("{BUSY}{}", b.name),
        (None, true) => format!("track {}", b.name),
        (None, false) => format!("switch {}", b.name),
    };
    let (m1, m2) = (if b.current { cur } else { " " }, if b.elsewhere.is_some() { wt } else { " " });
    let held = b
        .elsewhere
        .as_deref()
        .map(|d| format!(" {}", pick::clean(leaf(d))))
        .unwrap_or_default();
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
            copy: false,
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
            failed: false,
        };
        u.pick.help_len = help(g).len();
        u.sync();
        u
    }

    fn note(&mut self, t: &str) {
        let x = if self.ascii { "x" } else { "✕" };
        self.pick.header = format!("{x} {}", pick::clean(t));
    }

    /// An action that failed: its first non-blank line is the note, and the whole
    /// text takes the preview's place until the next key brings the graph back.
    pub fn failed(&mut self, msg: &str) {
        self.note(msg.lines().find(|l| !l.trim().is_empty()).unwrap_or(msg));
        self.pick.preview = Some(msg.lines().map(pick::clean).collect::<Vec<_>>().join("\n"));
        self.failed = true;
    }

    fn selected(&self) -> Option<&Branch> {
        self.branches.get(*self.pick.shown.get(self.pick.sel)?)
    }

    /// Shows the cached graph of the selection, or asks the worker for it. A failed
    /// action's message stays until a key ([`Ui::failed`]).
    fn sync(&mut self) {
        if self.failed {
            return;
        }
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
        self.failed = false;
        let out = if k.code == KeyCode::Enter && k.modifiers.contains(KeyModifiers::ALT) {
            self.create()
        } else {
            match self.pick.key(k, area) {
                Outcome::Choose(key) if key.starts_with(BUSY.as_bytes()) => {
                    let name = String::from_utf8_lossy(&key[BUSY.len()..]).into_owned();
                    let d = self.selected().and_then(|b| b.elsewhere.clone()).unwrap_or_default();
                    self.note(&format!("{name} is checked out in the worktree {}", leaf(&d)));
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

/// A checked action line: the verb and the name it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Switch(String),
    Track(String),
    Create(String),
}

/// An action line read as untrusted text: a verb from `switch track create`, one line, a
/// name git's own rule accepts, and for switch and track a ref that exists under
/// `refs/heads` or `refs/remotes`. `Err` is the line the popup shows.
pub fn parse(dir: &str, line: &[u8]) -> Result<Action, String> {
    let owned = String::from_utf8_lossy(line);
    let line: &str = &owned;
    let (verb, arg) = line.split_once(' ').unwrap_or((line, ""));
    let ns = match verb {
        "switch" => "heads",
        "track" => "remotes",
        "create" => "",
        _ => return Err(format!("cockpit: unexpected action: {line}")),
    };
    if line.contains('\n') || !branches::valid_name(dir, arg) {
        return Err(format!("cockpit: not a branch name: {arg}"));
    }
    if !ns.is_empty() && !branches::has_ref(dir, &format!("refs/{ns}/{arg}")) {
        return Err(format!("cockpit: no such branch: {arg}"));
    }
    let arg = arg.to_string();
    Ok(match verb {
        "switch" => Action::Switch(arg),
        "track" => Action::Track(arg),
        _ => Action::Create(arg),
    })
}

/// Runs a chosen line in `dir`'s tree, through [`gitsafe::git_write`]. `Ok` is what git
/// said it did (`Switched to branch …`); `Err` is the parse refusal or git's own words.
pub fn act(dir: &str, line: &[u8]) -> Result<String, String> {
    let run = |args: &[&str]| gitsafe::git_write(dir, args);
    match parse(dir, line)? {
        Action::Switch(b) => run(&["switch", &b]),
        Action::Track(r) => run(&["switch", "--track", &r]),
        Action::Create(n) => run(&["switch", "-c", &n]),
    }
}

/// Where the popup came from, for goto. `scripts/tmux-branch` hands both over in the
/// environment (#228).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Origin {
    /// `COCKPIT_BRANCH_PANE`: the pane the popup came from; a new window goes beside it
    pub pane: Option<String>,
    /// `COCKPIT_BRANCH_CLIENT`: the client that pressed the key (#360), the one goto moves
    pub client: Option<String>,
}

impl Origin {
    /// The two variables; a pane that is not `%<digits>` and an empty client are none.
    pub fn from_env() -> Self {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        Origin {
            pane: var("COCKPIT_BRANCH_PANE").filter(|p| crate::panel::is_pane(p)),
            client: var("COCKPIT_BRANCH_CLIENT").filter(|c| !c.chars().any(char::is_control)),
        }
    }
}

/// tmux reads `new-window -c` as a format, where `#(cmd)` runs cmd: `##` is a literal `#`.
/// A run of `#` before a `[` goes as it is, since tmux keeps `#[` and `##[` verbatim.
/// `scripts/lib/fmt.sh`'s `fesc`.
pub fn fesc(s: &str) -> String {
    let (mut out, mut run) = (String::new(), 0usize);
    for c in s.chars() {
        if c == '#' {
            run += 1;
            continue;
        }
        out.push_str(&"#".repeat(if c == '[' { run } else { 2 * run }));
        out.push(c);
        run = 0;
    }
    out + &"#".repeat(2 * run)
}

/// The deepest of `trees` holding `dir`, so a pane in a tree nested under the checkout
/// is not a pane of the checkout. `None` when no tree holds it.
pub fn owner_of<'a>(dir: &str, trees: &'a [String]) -> Option<&'a str> {
    let holds = |w: &str| dir == w || dir.strip_prefix(w).is_some_and(|r| r.starts_with('/'));
    trees.iter().map(String::as_str).filter(|w| holds(w)).max_by_key(|w| w.len())
}

/// Goes to the worktree at `path` (#303): the agent pane working in it if there is one,
/// any pane in it, else a new window there beside the origin pane. Never a cd of an
/// existing pane. `path` must be a tree git lists for `dir`'s repo and still a directory.
/// `Err` is the line the popup shows.
pub fn goto(dir: &str, path: &str, from: &Origin) -> Result<String, String> {
    let trees = branches::worktree_paths(dir);
    if !trees.iter().any(|w| w == path) {
        return Err(format!("cockpit: not a worktree of this repository: {path}"));
    }
    if !std::path::Path::new(path).is_dir() {
        return Err(format!("cockpit: {path} is gone · git worktree prune"));
    }
    if data::readonly() {
        return Err(format!("cockpit: COCKPIT_READONLY is set; no goto to {path}"));
    }
    let panes = data::pane_dirs().unwrap_or_default();
    let mine = |p: &&data::PaneDir| owner_of(&p.dir, &trees) == Some(path);
    let found = panes.iter().filter(mine).find(|p| p.agent).or_else(|| panes.iter().find(mine));
    if let Some(p) = found {
        let id = p.id.as_str();
        // fails without a client; the selection below still applies. Bare, tmux would move
        // the most recently active client (#360)
        match &from.client {
            Some(c) => data::tmux_write(&["switch-client", "-c", c, "-t", id]),
            None => data::tmux_write(&["switch-client", "-t", id]),
        };
        data::tmux_write(&["select-window", "-t", id]);
        data::tmux_write(&["select-pane", "-t", id]);
        return Ok(String::new());
    }
    // new-window -t takes a window, not a pane: beside the one the popup came from
    let is_win = |w: &str| {
        w.strip_prefix('@')
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    };
    let win = from
        .pane
        .as_deref()
        .and_then(|p| data::tmux(&["display-message", "-p", "-t", p, "#{window_id}"]));
    let win = win.map(|w| w.trim_end().to_string()).filter(|w| is_win(w));
    let at = fesc(path);
    let mut args = vec!["new-window"];
    if let Some(w) = &win {
        args.extend(["-a", "-t", w]);
    }
    args.extend(["-c", &at]);
    data::tmux_write(&args)
        .map(|_| String::new())
        .ok_or_else(|| format!("cockpit: could not open a window in {path}"))
}

/// Runs a chosen line: `goto <path>` through [`goto`], the rest through [`act`].
pub fn choose(dir: &str, line: &[u8], from: &Origin) -> Result<String, String> {
    match line.strip_prefix(b"goto ") {
        Some(p) => goto(dir, &String::from_utf8_lossy(p), from),
        None => act(dir, line),
    }
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
    // ↵ acts here (#228); pick's own row says it prints the row's key
    if at > 0 && rows[at - 1].0 == g.key_enter {
        rows[at - 1].1 = "switch to it, or its worktree".to_string();
    }
    rows.insert(at, (alt.to_string(), "create a branch named by the filter text".to_string()));
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

pub fn header(dir: &str, bs: &[Branch], g: &Glyphs) -> String {
    let cur = bs.iter().find(|b| b.current).map_or("detached HEAD", |b| b.name.as_str());
    let leaf = std::fs::canonicalize(dir)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
    format!("on {cur} {} {}", g.dot, leaf.unwrap_or_default())
}

/// Exit 0 with nothing on stdout once an action ran (git's own line goes to stderr), 1
/// for esc, 2 for a bad argument, no repository or no terminal. A failed action is a
/// note, and the picker stays.
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
    let from = Origin::from_env();
    let mut dirty = true;
    let mut acted: Option<String> = None;
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
                Outcome::Choose(line) => match choose(&dir, &line, &from) {
                    Ok(said) => {
                        acted = Some(said);
                        break Outcome::Cancel;
                    }
                    Err(e) => ui.failed(&e),
                },
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
    // git's own line, on the terminal the screen gave back, as the script printed it
    if let Some(said) = acted {
        if !said.is_empty() {
            eprintln!("{said}");
        }
        return 0;
    }
    pick::answer(&mut std::io::stdout().lock(), &out)
}

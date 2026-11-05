//! `cockpit-tui files <dir>`: the file browser (#363). It lists one folder in the
//! shared picker, walks the tree and prints one line: `view <path>`,
//! `open <path>` (the editor) or `copy <path>`. It never mutates;
//! `scripts/tmux-open-path --folder` checks the line again and acts.
//!
//! The row's key is the file's own name, as bytes, so the printed path is exact
//! whatever the name holds; the row's text is for the eye and drops control and
//! bidi characters. A name with a newline cannot ride one line, so it prints
//! nothing and says why.
use crate::chrome;
use crate::pick::{self, Opts, Outcome, Pick, Row};
use crate::theme::{Glyphs, Theme};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent};
use ignore::WalkBuilder;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use std::collections::HashSet;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// A folder with more entries than this lists its first ones and says so.
const MAX_ENTRIES: usize = 20_000;

pub struct Entry {
    pub name: Vec<u8>,
    pub is_dir: bool,
    /// Hidden by .gitignore, .ignore or git's excludes; only listed when asked for.
    pub ignored: bool,
}

pub struct Files {
    pub pick: Pick,
    pub dir: PathBuf,
    pub hidden: bool,
    /// `alt-i`: list what the ignore files hide, drawn dim.
    pub show_ignored: bool,
    /// The names in the folder shown that are ignored, for the dim.
    pub ignored: HashSet<Vec<u8>>,
    /// The folder as drawn in the header: cleaned, not yet cut to the width.
    pub label: String,
    cut: bool,
    note: Option<String>,
    g: Glyphs,
}

/// What `.gitignore`, `.ignore` and git's global excludes leave in `dir`: one
/// level, by the `ignore` crate (ripgrep's walker). `None` when the walk cannot say.
fn unignored(dir: &Path) -> Option<HashSet<Vec<u8>>> {
    let mut seen = HashSet::new();
    for e in WalkBuilder::new(dir).max_depth(Some(1)).hidden(false).build() {
        let e = e.ok()?;
        if e.depth() == 1 {
            seen.insert(e.file_name().as_bytes().to_vec());
        }
    }
    Some(seen)
}

/// The entries of `dir`, directories first, then by name without regard to case.
/// `.git` is never listed; dotfiles only with `hidden`; what the ignore files hide
/// only with `show_ignored`, marked. A symlink counts as what it points at; one
/// that points nowhere is a file. The one place the folder is read.
pub fn list_dir(dir: &Path, hidden: bool, show_ignored: bool) -> std::io::Result<Vec<Entry>> {
    let keep = unignored(dir);
    let mut v = Vec::new();
    for e in std::fs::read_dir(dir)? {
        let Ok(e) = e else { continue };
        let name = e.file_name().as_bytes().to_vec();
        if name == b".git" || (!hidden && name.first() == Some(&b'.')) {
            continue;
        }
        let ignored = keep.as_ref().is_some_and(|k| !k.contains(&name));
        if ignored && !show_ignored {
            continue;
        }
        let is_dir = std::fs::metadata(e.path()).is_ok_and(|m| m.is_dir());
        v.push(Entry { name, is_dir, ignored });
    }
    v.sort_by_cached_key(|e| (!e.is_dir, String::from_utf8_lossy(&e.name).to_lowercase(), e.name.clone()));
    Ok(v)
}

fn row(e: &Entry) -> Row {
    let slash = if e.is_dir { "/" } else { "" };
    Row {
        key: e.name.clone(),
        text: format!("{}{slash}", pick::clean(&String::from_utf8_lossy(&e.name))),
    }
}

fn label(dir: &Path) -> String {
    pick::clean(&String::from_utf8_lossy(dir.as_os_str().as_bytes()))
}

fn answer_line(verb: &str, path: &Path) -> Vec<u8> {
    [verb.as_bytes(), b" ", path.as_os_str().as_bytes()].concat()
}

impl Files {
    /// `dir` must be a folder that can be read; the error names it.
    pub fn new(dir: &Path, g: &Glyphs) -> Result<Self, String> {
        let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        let o = Opts {
            prompt: "files".into(),
            header: String::new(),
            preview: None,
            copy: true,
        };
        let mut f = Self {
            pick: Pick::new(Vec::new(), &o, None),
            label: label(&dir),
            dir,
            hidden: false,
            show_ignored: false,
            ignored: HashSet::new(),
            cut: false,
            note: None,
            g: *g,
        };
        f.pick.help_len = help(g).len();
        let dir = f.dir.clone();
        let (rows, cut) = f.read(&dir).map_err(|e| format!("cannot read {}: {e}", f.dir.display()))?;
        f.cut = cut;
        f.show(rows, None, Rect::default());
        Ok(f)
    }

    fn read(&mut self, dir: &Path) -> std::io::Result<(Vec<Row>, bool)> {
        let mut es = list_dir(dir, self.hidden, self.show_ignored)?;
        let cut = es.len() > MAX_ENTRIES;
        es.truncate(MAX_ENTRIES);
        self.ignored = es.iter().filter(|e| e.ignored).map(|e| e.name.clone()).collect();
        Ok((es.iter().map(row).collect(), cut))
    }

    /// New rows with an empty filter; the selection lands on `select` when it is there.
    fn show(&mut self, rows: Vec<Row>, select: Option<&[u8]>, area: Rect) {
        let p = &mut self.pick;
        p.rows = rows;
        p.query.clear();
        p.shown = pick::filter(&p.rows, "");
        p.sel = select.and_then(|k| p.shown.iter().position(|&i| p.rows[i].key == k)).unwrap_or(0);
        p.top = 0;
        p.prompt = format!(
            "files{}{}",
            if self.hidden { " +." } else { "" },
            if self.show_ignored { " +i" } else { "" }
        );
        p.clamp(area);
    }

    fn say(&mut self, t: &str) {
        let x = if self.g.ascii { "x" } else { "✕" };
        self.note = Some(format!("{x} {}", pick::clean(t)));
    }

    /// The same folder with the hidden files on or off, on the same row.
    fn reload(&mut self, area: Rect) {
        let kept = self.pick.shown.get(self.pick.sel).map(|&i| self.pick.rows[i].key.clone());
        let dir = self.dir.clone();
        match self.read(&dir) {
            Ok((rows, cut)) => {
                self.cut = cut;
                self.show(rows, kept.as_deref(), area);
            }
            Err(e) => self.say(&format!("cannot read this folder: {e}")),
        }
    }

    fn go(&mut self, to: PathBuf, select: Option<Vec<u8>>, area: Rect) {
        match self.read(&to) {
            Ok((rows, cut)) => {
                self.cut = cut;
                self.label = label(&to);
                self.dir = to;
                self.show(rows, select.as_deref(), area);
            }
            Err(e) => self.say(&format!(
                "cannot open {}: {e}",
                pick::clean(&String::from_utf8_lossy(to.file_name().unwrap_or(OsStr::new("")).as_bytes()))
            )),
        }
    }

    fn up(&mut self, area: Rect) {
        if let Some(parent) = self.dir.parent() {
            let from = self.dir.file_name().map(|n| n.as_bytes().to_vec());
            self.go(parent.to_path_buf(), from, area);
        }
    }

    /// The selected row's entry as a path, and whether it is a folder.
    fn selected(&self) -> Option<(PathBuf, bool)> {
        let r = &self.pick.rows[*self.pick.shown.get(self.pick.sel)?];
        Some((self.dir.join(OsStr::from_bytes(&r.key)), r.text.ends_with('/')))
    }

    /// The line to print for the selection, unless the name cannot ride one line.
    fn leaf(&mut self, verb: &str) -> Outcome {
        let Some((path, _)) = self.selected() else {
            return Outcome::Continue;
        };
        if path.as_os_str().as_bytes().contains(&b'\n') {
            self.say("a newline in the name: it cannot be sent on one line");
            return Outcome::Continue;
        }
        if path.to_string_lossy().chars().any(|c| c.is_control() || pick::is_bidi(c)) {
            self.say("a control or bidi character in the path: it cannot be sent safely");
            return Outcome::Continue;
        }
        Outcome::Choose(answer_line(verb, &path))
    }

    /// alt-e: the editor for a file; a folder has nothing to edit.
    fn edit(&mut self) -> Outcome {
        match self.selected() {
            Some((_, false)) => self.leaf("open"),
            _ => Outcome::Continue,
        }
    }

    /// ↵ views a file (alt-e opens it in the editor) and enters a folder; ← and ⌫
    /// on an empty filter go up; `.` on an empty filter shows or hides dotfiles;
    /// everything else is the picker's.
    /// A note is cleared by the next key.
    pub fn key(&mut self, k: KeyEvent, area: Rect) -> Outcome {
        self.note = None;
        let plain = k.modifiers.difference(KeyModifiers::SHIFT).is_empty();
        let walk = self.pick.help.is_none() && !pick::too_small(area);
        let out = match k.code {
            KeyCode::Enter if walk => match self.selected() {
                Some((p, true)) => {
                    self.go(p, None, area);
                    Outcome::Continue
                }
                _ => self.leaf("view"),
            },
            KeyCode::Char('e') if walk && k.modifiers.contains(KeyModifiers::ALT) => self.edit(),
            KeyCode::Char('c') if walk && k.modifiers.contains(KeyModifiers::ALT) => self.leaf("copy"),
            KeyCode::Left if walk && plain => {
                self.up(area);
                Outcome::Continue
            }
            KeyCode::Backspace if walk && plain && self.pick.query.is_empty() => {
                self.up(area);
                Outcome::Continue
            }
            KeyCode::Char('.') if walk && plain && self.pick.query.is_empty() => {
                self.hidden = !self.hidden;
                self.reload(area);
                Outcome::Continue
            }
            KeyCode::Char('i') if walk && k.modifiers.contains(KeyModifiers::ALT) => {
                self.show_ignored = !self.show_ignored;
                self.reload(area);
                Outcome::Continue
            }
            // pick's own Enter and alt-c answer with the bare name, never a line
            _ => match self.pick.key(k, area) {
                Outcome::Cancel => Outcome::Cancel,
                _ => Outcome::Continue,
            },
        };
        self.pick.clamp(area);
        let g = self.g;
        self.fit(area.width, &g);
        out
    }

    pub fn mouse(&mut self, m: MouseEvent, area: Rect) {
        self.pick.mouse(m, area);
    }

    /// Sets the header for a screen `w` wide: the path cut on the left, so the
    /// folder's own name stays, or a note until the next key.
    pub fn fit(&mut self, w: u16, g: &Glyphs) {
        let room = (w as usize).saturating_sub(2);
        let more = if self.cut { "  (first rows)" } else { "" };
        self.pick.header = match &self.note {
            Some(n) => g.trunc(n, room),
            None => format!("{}{more}", g.trunc_left(&self.label, room.saturating_sub(more.chars().count()))),
        };
    }
}

pub fn hints(g: &Glyphs, w: usize, hidden: bool, ignored: bool) -> Vec<(String, String)> {
    let h = |k: &str, v: &str| (k.to_string(), v.to_string());
    let left = g.key_left;
    let fitted = chrome::fit(
        vec![
            h(g.key_enter, "view"),
            h(left, "up"),
            h("alt-e", "edit"),
            h(".", if hidden { "dotfiles on" } else { "dotfiles off" }),
            h("alt-i", if ignored { "ignored on" } else { "ignored off" }),
            h("alt-c", "copy"),
            h("F1", "keys"),
            h("esc", "close"),
        ],
        w,
        3,
    );
    if fitted.iter().any(|(k, _)| k == g.key_enter) {
        fitted
    } else {
        vec![h(g.key_enter, "view"), h("esc", "close")]
    }
}

/// pick's F1 help with this screen's keys.
pub fn help(g: &Glyphs) -> Vec<(String, String)> {
    let mut rows = pick::help_rows_for(g, true);
    let left = g.key_left;
    if let Some(r) = rows.iter_mut().find(|(k, _)| k == g.key_enter) {
        r.1 = "view the file (print its path), or go into the folder".into();
    }
    let at = rows.iter().position(|(k, _)| k == g.key_enter).map_or(rows.len(), |i| i + 1);
    rows.insert(
        at,
        (format!("{left}  {}", g.key_bksp), "up a folder (⌫ only on an empty filter)".into()),
    );
    rows.insert(at + 1, (".".into(), "show or hide dotfiles (on an empty filter)".into()));
    rows.insert(at + 2, ("alt-i".into(), "show or hide what .gitignore hides, drawn dim".into()));
    let edit = "open the file in the editor: print its path, exit";
    rows.insert(at + 3, ("alt-e".into(), edit.into()));
    if let Some(r) = rows.iter_mut().find(|(k, _)| k == "alt-c") {
        r.1 = "copy: print this row's path, exit".into();
    }
    rows
}

/// pick's row, dim when the ignore files would hide it.
fn dim_row(u: &Files, th: &Theme, r: &Row, selected: bool, w: usize) -> Line<'static> {
    let text = th.g.trunc(&r.text, w);
    let mut style = Style::new().fg(if u.ignored.contains(&r.key) { th.faint } else { th.value });
    if selected {
        style = style.add_modifier(Modifier::BOLD);
        return Line::from(vec![
            Span::styled(th.g.active, Style::new().fg(th.accent)),
            Span::styled(format!(" {text}"), style),
        ]);
    }
    Line::from(vec![Span::raw(" "), Span::styled(format!(" {text}"), style)])
}

pub fn draw(f: &mut ratatui::Frame, area: Rect, u: &Files, th: &Theme) {
    let row = |r: &Row, sel: bool, w: usize| dim_row(u, th, r, sel, w);
    let look = pick::Look {
        title: Some("files"),
        row: Some(&row),
        hints: Some(hints(&th.g, area.width as usize, u.hidden, u.show_ignored)),
        help: Some(help(&th.g)),
        empty: u.pick.query.is_empty().then(|| "empty folder".to_string()),
    };
    pick::render_with(f, area, &u.pick, th, &look);
}

const USAGE: &str = "cockpit-tui files <dir>";

/// Exit 0 with `view <path>`, `open <path>` or `copy <path>` on stdout, 1 for esc, 2 for a bad
/// argument, a folder that cannot be read or no terminal.
pub fn main(args: &[String]) -> i32 {
    let [dir] = args else {
        eprintln!("cockpit-tui: files: needs exactly one folder\n{USAGE}");
        return 2;
    };
    let th = Theme::load();
    let mut f = match Files::new(Path::new(dir), &th.g) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("cockpit-tui: files: {e}\n{USAGE}");
            return 2;
        }
    };
    let mut term = match pick::open_screen("files") {
        Ok(t) => t,
        Err(c) => return c,
    };
    let out = loop {
        let area = pick::screen(&term);
        f.pick.clamp(area);
        f.fit(area.width, &th.g);
        if term.draw(|fr| draw(fr, fr.area(), &f, &th)).is_err() {
            break Outcome::Cancel;
        }
        match event::read() {
            Ok(Event::Key(k)) if k.kind != KeyEventKind::Release => match f.key(k, pick::screen(&term)) {
                Outcome::Continue => {}
                o => break o,
            },
            Ok(Event::Mouse(m)) => f.mouse(m, pick::screen(&term)),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break Outcome::Cancel,
        }
    };
    drop(term);
    pick::restore_tty();
    pick::answer(&mut std::io::stdout().lock(), &out)
}

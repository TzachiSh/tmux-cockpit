//! The panel's state machine: views, keys, and the request round trip. No
//! terminal here, so every transition is a unit test away.
use super::action::{Hidden, hide_body, sanitize, state_body};
use super::capture::{Capture, REPO_BUDGET, SSH_LIKE, Session, is_local, session_state, toplevel, window_kind};
use super::envelope::{Flow, OutType, Role, Turn, build_counted};
use super::parse::{self, Opt, Reply, Step};
use super::prompts::{self, Ctx};
use super::redact::redact;
use super::request::{Call, Fail, Profile, Request, read_dir_ok};
use super::route::{Route, route};
use super::view::{TABLET_MIN_H, TABLET_MIN_W};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

pub const MODELS: [&str; 4] = ["auto", "haiku", "sonnet", "opus"];
pub const OUT_CYCLE: [OutType; 7] = [
    OutType::Auto,
    OutType::Prompt,
    OutType::Command,
    OutType::Steps,
    OutType::Text,
    OutType::Format,
    OutType::Hint,
];
pub const MAX_DRAFT: usize = 8000;
/// Past this the Compose head counts the draft against MAX_DRAFT.
pub const DRAFT_COUNT: usize = 6000;
/// tmux 3.8 drops, silently, an OSC 52 whose base64 tops `input-buffer-size`
/// (1 MiB): 786,000 bytes landed, 786,432 did not. Kept under it.
pub const COPY_MAX: usize = 768_000;
/// Termux is said to cut an OSC string near 8 KB (unconfirmed): warned, not refused.
pub const COPY_TERMUX: usize = 6_000;
/// A tap this soon after the view changed is the second half of a double tap.
pub const TAP_GUARD_MS: u64 = 300;
/// A resumed panel's first view: the close, the paste and the reopen fit inside
/// a slow double tap, and the reopened Steps put a button under the same finger.
pub const RESUME_GUARD_MS: u64 = 800;
/// An explore-first write's caps (#416): the prompt and a few reads on sonnet fit, and
/// more reading than that is the agent's task, so the call ends rather than keeps the user.
pub const EXPLORE_BUDGET_USD: f64 = 0.15;
pub const EXPLORE_TIMEOUT: Duration = Duration::from_secs(90);
const ROUNDS_WRITE: usize = 3;
const ROUNDS_EXPLAIN: usize = 6;
const BEST_GUESS: &str = "(no preference - use your best guess and state the assumption)";
/// The one retry a write gets when it asks with no questions left: still no
/// guess at what only the user knows (#503).
const WRITE_NOW: &str = concat!(
    "(no questions left: write it now without guessing what only I know: have Claude Code ",
    "come back to me with the options for it first, or leave a placeholder for it)"
);

pub const QUICK: [(&str, &str); 4] = [
    ("What is it doing?", "What is happening here right now?"),
    ("What should happen next?", "What should happen next, and what do I type or tap?"),
    ("Is something wrong?", "Is something wrong here? How do I fix it?"),
    (
        "Explain everything",
        "(no question) What is on this screen and what should I do next?",
    ),
];

/// Programs that take a bracketed paste as one block, with [`SSH_LIKE`]: the
/// remote shell gets it. Anywhere else each line of a paste may run alone.
const BRACKETED_OK: [&str; 8] = ["bash", "zsh", "fish", "claude", "node", "tmux", "vim", "nvim"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tile {
    WritePrompt,
    WriteCommand,
    Compose,
    Explain,
    /// Flips pc and tablet for this run of the panel.
    Mode,
    Close,
}

/// What a tap on a recorded hit rect does: the key the element stands for, or
/// an entry chosen by its index (`Pick` also presses Enter on it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tap {
    Key(KeyEvent),
    Pick(usize),
    Select(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputKind {
    Other,
    Explain,
    FollowUp,
    /// A correction to the result's `Assumed:` line of this index.
    Correct(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub enum View {
    Home {
        sel: usize,
    },
    Compose,
    Busy,
    Question {
        sel: usize,
        /// `None` follows the selection; PgUp lets the question's start be read.
        scroll: Option<usize>,
    },
    Input {
        kind: InputKind,
        text: String,
        /// The cursor: a byte offset into `text`, on a char boundary.
        cur: usize,
        back: Box<View>,
    },
    Result {
        label: String,
        text: String,
        scroll: usize,
    },
    Steps {
        label: String,
        items: Vec<Step>,
        sel: usize,
        /// One per step: inserted or sent in an earlier round of this popup.
        done: Vec<bool>,
    },
    ExplainAsk {
        sel: usize,
    },
    Answer {
        scroll: usize,
        /// One Esc pressed: the next drops the conversation.
        armed: bool,
    },
    Confirm {
        text: String,
        send: bool,
        warn: Vec<String>,
        back: Box<View>,
        scroll: usize,
    },
    Error {
        line: String,
        /// One line on what to do about it, when the failure is a known one.
        hint: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

/// One line above the body; it lasts until the view changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub level: Level,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    /// `paste` inserts; `send` inserts and presses Enter.
    pub verb: &'static str,
    pub text: String,
}

/// The Steps view across a paste: the wrapper runs the action and opens the
/// panel again on this, in the same popup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resume {
    pub label: String,
    pub items: Vec<Step>,
    /// The step the action was taken on.
    pub sel: usize,
    pub done: Vec<bool>,
    pub verb: &'static str,
}

/// What a call carried when its reply came: the turn in flight, and the rounds
/// to give back if it is dropped.
#[derive(Clone, Copy)]
struct Carried {
    pending: bool,
    from: Option<usize>,
}

/// One request's conversation: the question rounds and, for Explain, the log.
pub struct Run {
    pub flow: Flow,
    pub turns: Vec<Turn>,
    pub rounds: usize,
    pub max: usize,
    pub qstack: Vec<(String, Vec<Opt>, Vec<Turn>)>,
    pub question: String,
    pub options: Vec<Opt>,
    /// Explain's transcript: `(true, text)` is the user, `false` Claude.
    pub log: Vec<(bool, String)>,
    pub ready: Option<String>,
    /// The model the call goes to, and the rule or choice that picked it.
    pub model: String,
    pub why: String,
    /// The last turn and log entry were added for the call in flight.
    pub pending_turn: bool,
    /// The rounds before a best guess or a correction, so a cancelled one can be undone.
    pub guessed_from: Option<usize>,
}

pub struct App {
    pub pane: String,
    pub cap: Capture,
    pub session: Option<Session>,
    pub view: View,
    pub draft: String,
    /// The draft's cursor, as [`View::Input`]'s `cur`.
    pub cur: usize,
    pub out: OutType,
    pub model: usize,
    /// ^R on Compose: a write may read the pane's folder first (#416). Kept
    /// for the panel's life and across a hide, never on a file of its own.
    pub explore: bool,
    /// The pane, as captured when the panel opened, runs where its folder is:
    /// [`is_local`]. A resume's older command does not change it.
    local: bool,
    /// [`App::explore_dir`]'s answer, worked out once: Busy asks it every frame.
    explore_root: std::cell::OnceCell<Result<PathBuf, String>>,
    pub run: Option<Run>,
    /// Set by a transition, cleared by [`App::launch`], which owns the fork.
    pub pending: bool,
    pub req: Option<Request>,
    pub started: Option<Instant>,
    /// Secrets the last call's envelope hid (screen and turns), and the screen's alone.
    pub hidden: usize,
    pub screen_hidden: usize,
    /// The call's limit, from [`Call`] at launch; zero until a call goes out.
    pub timeout: Duration,
    pub busy_back: Option<View>,
    pub note: Option<Note>,
    /// Counts [`App::say`] calls, so a key can tell a note it set from an old one.
    said: u64,
    pub action: Option<Action>,
    /// Opened again by the wrapper after an action: keys typed during the
    /// handover were meant for a screen not yet drawn.
    pub reopened: bool,
    /// Set with an action taken from Steps: what `<action>.state` carries.
    pub resume: Option<Resume>,
    /// ^Z: out with no action, and the whole state in `<action>.state` (#278).
    pub hide: bool,
    /// A panel is hidden on this pane already (an ask left it so): ^Z cannot hide
    /// this one too, as the pane keeps one.
    pub hide_taken: bool,
    pub quit: bool,
    /// The step this run was resumed for, and its verb: the mark says what was done.
    pub last_step: Option<(usize, &'static str)>,
    /// Body rows on screen and the furthest scroll, from the last frame.
    pub room: usize,
    pub bottom: usize,
    /// The `?` overlay's first row while open, measured by [`App::fit`]: arrows
    /// scroll it, any other key only closes it.
    pub help: Option<usize>,
    /// The last frame was under the panel's floor: only esc, q and ^C act.
    pub tiny: bool,
    /// The last frame's columns and rows, once one is drawn.
    pub size: Option<(u16, u16)>,
    /// Tablet presentation: mouse capture, tall tiles, tap buttons.
    pub tablet: bool,
    /// Text for the event loop to put on the clipboard (OSC 52).
    pub copy: Option<String>,
    /// Under Termux, whose OSC limit is lower: see [`COPY_TERMUX`].
    pub termux: bool,
    /// What the last frame showed and when, in the loop's milliseconds, for the tap guard.
    seen: Option<(std::mem::Discriminant<View>, bool, bool)>,
    seen_at: u64,
    /// The guard for the first view: longer after a resume.
    first_guard: u64,
    guard: u64,
}

impl App {
    pub fn new(pane: &str, cap: Capture) -> App {
        let screen_hidden = redact(&cap.screen).1;
        let local = is_local(&cap.cmd);
        App {
            pane: pane.into(),
            session: session_state(&cap.screen, &cap.name, &cap.cmd),
            cap,
            view: View::Home { sel: 0 },
            draft: String::new(),
            cur: 0,
            out: OutType::Auto,
            model: 0,
            explore: false,
            local,
            explore_root: std::cell::OnceCell::new(),
            run: None,
            pending: false,
            req: None,
            started: None,
            hidden: 0,
            screen_hidden,
            timeout: Duration::ZERO,
            busy_back: None,
            note: None,
            said: 0,
            action: None,
            resume: None,
            hide: false,
            hide_taken: false,
            reopened: false,
            quit: false,
            last_step: None,
            room: 0,
            bottom: 0,
            help: None,
            tiny: false,
            size: None,
            tablet: false,
            copy: None,
            termux: false,
            seen: None,
            seen_at: 0,
            first_guard: TAP_GUARD_MS,
            guard: TAP_GUARD_MS,
        }
    }

    /// Back on the Steps a resume file carried, after the wrapper ran its
    /// action. `last` is `ok`, or `failed:<why>` from the wrapper.
    pub fn resumed(&mut self, cmd: String, r: Resume, last: &str) {
        // the steps were written for this command: the wrapper's guard checks it
        self.cap.cmd = cmd;
        self.session = session_state(&self.cap.screen, &self.cap.name, &self.cap.cmd);
        let Resume {
            label,
            items,
            mut sel,
            mut done,
            verb,
        } = r;
        let n = sel + 1;
        self.first_guard = RESUME_GUARD_MS;
        match last.strip_prefix("failed:") {
            None if last == "ok" => {
                done[sel] = true;
                self.last_step = Some((sel, verb));
                let what = if verb == "send" { "sent" } else { "inserted" };
                self.say(Level::Info, format!("step {n} {what}"));
                if let Some(next) = (sel + 1..items.len()).find(|&i| !done[i] && !items[i].cmd.is_empty()) {
                    sel = next;
                }
            }
            Some(why) => {
                let why: String = sanitize(why).replace('\n', " ").chars().take(200).collect();
                self.say(Level::Error, format!("step {n} was not sent: {why}"));
            }
            None => self.say(Level::Error, format!("step {n}: no word on how it went")),
        }
        self.view = View::Steps { label, items, sel, done };
    }

    /// Back where ^Z left it. Nothing is inserted or sent: the draft and the
    /// reply wait for a key, and the first view gets the resume's tap guard.
    pub fn unhide(&mut self, h: Hidden) {
        // as in a steps resume: the wrapper's guard checks what it was written for
        let now = std::mem::replace(&mut self.cap.cmd, h.cmd);
        self.session = session_state(&self.cap.screen, &self.cap.name, &self.cap.cmd);
        (self.view, self.busy_back, self.run) = (h.view, h.busy_back, h.run);
        (self.draft, self.cur, self.out, self.model) = (h.draft, h.cur, h.out, h.model);
        // the pane may run ssh by now: its folder is then the client's, never read
        self.explore = h.explore && self.explore_dir().is_ok();
        let last = self.tiles().len() - 1;
        if let View::Home { sel } = &mut self.view {
            *sel = (*sel).min(last);
        }
        self.first_guard = RESUME_GUARD_MS;
        let mut back = "back where you left it".to_string();
        let moved = now != self.cap.cmd;
        if moved {
            let was = &self.cap.cmd;
            back.push_str(&format!("; the pane now runs {now}, not {was}, so a paste is refused"));
        }
        let dropped = h.explore && !self.explore;
        if dropped {
            back.push_str("; explore is off, as this pane's folder cannot be read");
        }
        self.say(if moved || dropped { Level::Warn } else { Level::Info }, back);
    }

    /// The ★ tile first: what this window most likely wants. When that is
    /// Compose itself, it is not listed twice.
    pub fn tiles(&self) -> Vec<(Tile, &'static str, &'static str)> {
        const COMPOSE: (Tile, &str, &str) = (Tile::Compose, "Compose", "type once: fix spelling, write, or insert");
        let star = match window_kind(&self.cap.name, &self.cap.cmd) {
            "claude" => Some((Tile::WritePrompt, "Write a prompt", "a task for Claude Code from your rough words")),
            "shell" => Some((Tile::WriteCommand, "Write a command", "describe it, Claude writes the command")),
            _ => None,
        };
        let mode = if self.tablet {
            (Tile::Mode, "PC mode", "keys only: the terminal selects text again")
        } else {
            (Tile::Mode, "Tablet mode", "big tiles, tap buttons, the mouse")
        };
        let mut t: Vec<_> = star.into_iter().collect();
        t.extend([
            COMPOSE,
            (Tile::Explain, "Explain", "what is on this screen, and what next"),
            mode,
            (Tile::Close, "Close", ""),
        ]);
        t
    }

    pub fn screen_lines(&self) -> usize {
        if self.cap.screen.is_empty() {
            0
        } else {
            self.cap.screen.split('\n').count()
        }
    }

    pub fn write_route(&self) -> Route {
        route(
            &Flow::Write {
                draft: self.draft.clone(),
                out: self.out,
            },
            MODELS[self.model],
        )
    }

    /// The multi-line hazard of pasting `text` into this pane, if any.
    pub fn line_warning(&self, text: &str) -> Option<String> {
        let n = text.trim_end_matches('\n').split('\n').count();
        (n > 1 && !bracketed_ok(&self.cap.cmd)).then(|| format!("⚠ {n} lines – '{}' may run each line", self.cap.cmd))
    }

    pub fn key(&mut self, k: KeyEvent) {
        if k.kind != KeyEventKind::Press {
            return;
        }
        let (was, said) = (std::mem::discriminant(&self.view), self.said);
        self.handle(k);
        self.settle(was, said);
    }

    /// A tap on a hit rect: the key it stands for, or an entry by index. The
    /// selection moves first, so a `Pick` is the Enter the keys would press.
    /// Dropped within [`TAP_GUARD_MS`] of a new view: the cell under the finger
    /// may now hold the confirming button. `now` is the loop's clock, like `tick`.
    pub fn tap(&mut self, t: Tap, now: u64) {
        // under the floor too: the card shows nothing a tap could be meant for
        if now.saturating_sub(self.seen_at) < self.guard || self.tiny {
            return;
        }
        let i = match t {
            Tap::Key(k) => return self.key(k),
            Tap::Pick(i) | Tap::Select(i) => i,
        };
        if self.help.is_some() {
            return;
        }
        match &mut self.view {
            View::Home { sel } | View::ExplainAsk { sel } | View::Steps { sel, .. } => *sel = i,
            View::Question { sel, scroll } => (*sel, *scroll) = (i, None),
            _ => return,
        }
        if matches!(t, Tap::Pick(_)) {
            self.key(KeyEvent::from(KeyCode::Enter));
        }
    }

    /// Called after each frame is drawn: a view that differs from the last
    /// frame's starts the tap guard. The selection moving is not a new view.
    pub fn frame(&mut self, now: u64) {
        let armed = matches!(self.view, View::Answer { armed: true, .. });
        let seen = Some((std::mem::discriminant(&self.view), armed, self.help.is_some()));
        if seen != self.seen {
            self.guard = if self.seen.is_none() { self.first_guard } else { TAP_GUARD_MS };
            (self.seen, self.seen_at) = (seen, now);
        }
    }

    /// The wheel is Up and Down, except in a text box, where those keys move the
    /// cursor (Compose) or do nothing: a scroll is never read as an edit.
    pub fn wheel(&mut self, up: bool) {
        if self.help.is_none() && matches!(self.view, View::Compose | View::Input { .. }) {
            return;
        }
        self.key(KeyEvent::from(if up { KeyCode::Up } else { KeyCode::Down }));
    }

    /// Queues `text` for the clipboard; the event loop writes it and says so.
    /// Cleaned like a paste: an ESC copied out would be a keystroke wherever it lands.
    fn copy_out(&mut self, text: &str) {
        let text = sanitize(text);
        let n = text.chars().count();
        if text.is_empty() {
            return self.say(Level::Info, "nothing to copy here");
        }
        if text.len() > COPY_MAX {
            return self.say(Level::Warn, format!("too long to copy: {n} chars"));
        }
        // the terminal never answers an OSC 52: sent is all that is known
        let sent = format!("sent {n} chars to the clipboard (needs tmux set-clipboard on)");
        if self.termux && text.len() > COPY_TERMUX {
            self.say(Level::Warn, format!("{sent}; Termux may cut it near 8 KB"));
        } else {
            self.say(Level::Info, sent);
        }
        self.copy = Some(text);
    }

    /// A note belongs to the view it was said on: a move to another view
    /// drops it, unless the move itself said something.
    fn settle(&mut self, was: std::mem::Discriminant<View>, said: u64) {
        if std::mem::discriminant(&self.view) != was && self.said == said {
            self.note = None;
        }
    }

    pub fn say(&mut self, level: Level, text: impl Into<String>) {
        self.note = Some(Note { level, text: text.into() });
        self.said += 1;
    }

    /// The MAX_DRAFT cap's note, or none when nothing was turned away.
    fn cap_note(&mut self, dropped: usize) {
        if dropped > 0 {
            self.say(Level::Warn, format!("the box holds {MAX_DRAFT} characters: {dropped} left out"));
        } else {
            self.note = None;
        }
    }

    fn handle(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && k.code == KeyCode::Char('c') {
            return self.exit();
        }
        // before the card and the overlay: hiding loses nothing, whatever is on screen
        if ctrl && k.code == KeyCode::Char('z') {
            return self.hide_key();
        }
        if let Some(top) = self.help.take() {
            self.help = self.scrolled(k.code, top);
            return;
        }
        // esc and q act as on the view under the card, so nothing is lost unasked;
        // a key whose effect nobody can see is dropped, q typed into a box too
        let boxed = matches!(self.view, View::Compose | View::Input { .. });
        if self.tiny && !(k.code == KeyCode::Esc || (k.code == KeyCode::Char('q') && !boxed)) {
            return;
        }
        // q in a confirm is "no" and on an answer is esc: a reflex q must not lose the answer
        let typing = matches!(
            self.view,
            View::Compose | View::Input { .. } | View::Confirm { .. } | View::Answer { .. }
        );
        if !typing && k.code == KeyCode::Char('q') {
            return self.exit();
        }
        // `?` is text in a box, so F1 opens the overlay everywhere
        if k.code == KeyCode::F(1) || (!boxed && k.code == KeyCode::Char('?')) {
            self.help = Some(0);
            return;
        }
        // the views that own text are moved out, never cloned: each handler puts its view back
        match &self.view {
            View::Home { sel } => self.home_key(k, *sel),
            View::Compose => self.compose_key(k, ctrl),
            View::Busy => {
                if k.code == KeyCode::Esc {
                    self.cancel();
                }
            }
            View::Question { sel, scroll } => self.question_key(k, *sel, *scroll),
            View::ExplainAsk { sel } => self.explain_ask_key(k, *sel),
            View::Answer { scroll, armed } => self.answer_key(k, *scroll, *armed),
            View::Input { .. } | View::Result { .. } | View::Steps { .. } | View::Confirm { .. } => {
                match std::mem::replace(&mut self.view, View::Busy) {
                    View::Input { kind, text, cur, back } => self.input_key(k, ctrl, kind, text, cur, *back),
                    View::Result { text, scroll, label } => self.result_key(k, text, scroll, label),
                    View::Steps { label, items, sel, done } => self.steps_key(k, label, items, sel, done),
                    View::Confirm {
                        text,
                        send,
                        back,
                        scroll,
                        warn,
                    } => self.confirm_key(k, text, send, warn, back, scroll),
                    _ => unreachable!("matched above"),
                }
            }
            View::Error { .. } => match k.code {
                KeyCode::Char('r') => self.retry(),
                KeyCode::Esc => {
                    self.drop_pending();
                    self.view = self.busy_back.take().unwrap_or(View::Home { sel: 0 });
                }
                _ => {}
            },
        }
    }

    /// A bracketed paste: the Compose box takes it whole, a one-line box joins
    /// its lines with spaces. Anywhere else it is dropped, never read as keys.
    pub fn paste(&mut self, s: &str) {
        if self.tiny {
            return;
        }
        let dropped = match &mut self.view {
            View::Compose => put(&mut self.draft, &mut self.cur, &pasted(s, true)),
            View::Input { text, cur, .. } => put(text, cur, &pasted(s, false)),
            _ => return,
        };
        self.cap_note(dropped);
    }

    /// The last frame's body rows and line count, so a stored offset is always
    /// one the screen can show: Up at the end moves at once.
    pub fn fit(&mut self, total: usize, room: usize) {
        self.room = room;
        self.bottom = total.saturating_sub(room);
        if let Some(top) = &mut self.help {
            return *top = (*top).min(self.bottom);
        }
        if let View::Result { scroll, .. } | View::Answer { scroll, .. } | View::Confirm { scroll, .. } = &mut self.view {
            *scroll = (*scroll).min(self.bottom);
        }
        // a resize that fits it all unpins: nothing is left to read from the start
        if let View::Question { scroll: s @ Some(_), .. } = &mut self.view {
            *s = s.map(|s| s.min(self.bottom)).filter(|_| self.bottom > 0);
        }
    }

    /// The last frame's size against the floor. The card hides the overlay, so it
    /// closes: the card's esc must go back, not close what nobody can see.
    pub fn shrunk(&mut self, tiny: bool) {
        self.tiny = tiny;
        if tiny {
            self.help = None;
        }
    }

    /// The offset after a scroll key, or `None` for any other key.
    fn scrolled(&self, code: KeyCode, scroll: usize) -> Option<usize> {
        crate::chrome::scroll(code, scroll, self.bottom, self.room)
    }

    /// Out with no action, the call killed: a hangup, or a loop that broke.
    pub fn stop(&mut self) {
        self.action = None;
        self.resume = None;
        self.hide = false;
        self.exit();
    }

    /// ^Z: out with no action and the state kept for the next `prefix f` on
    /// this pane. Never while a call is out: `claude -p` cannot outlive the
    /// process, and running it again on the way back would be a second paid call.
    fn hide_key(&mut self) {
        if self.view == View::Busy {
            return self.say(Level::Warn, "a call is out: esc cancels it, then ^Z hides");
        }
        if self.hide_taken {
            let why = "a panel is already hidden on this pane: q closes this one";
            return self.say(Level::Warn, why);
        }
        // u64::MAX: the longest timestamp the file can carry
        if hide_body(self, u64::MAX).is_none() {
            return self.say(Level::Warn, "too much to keep hidden: q closes the panel");
        }
        self.help = None;
        self.hide = true;
        self.exit();
    }

    fn exit(&mut self) {
        if let Some(mut r) = self.req.take() {
            r.cancel();
        }
        self.quit = true;
    }

    /// A paste is only bracketed when the program asked for it, so a multi-line
    /// insert into anything else can run line one: that goes through a confirm.
    /// An open menu takes typed text as its answer, so that confirms too.
    fn insert(&mut self, text: String, back: View) {
        if self.line_warning(&text).is_some() || self.session == Some(Session::WaitingChoice) {
            self.confirm(text, false, back);
        } else {
            self.act("paste", text, back);
        }
    }

    fn confirm(&mut self, text: String, send: bool, back: View) {
        let warn = self.warnings(&text, send);
        self.view = View::Confirm {
            text,
            send,
            warn,
            back: Box::new(back),
            scroll: 0,
        };
    }

    /// `back` is the view the action was taken from, kept for a resume.
    fn act(&mut self, verb: &'static str, text: String, back: View) {
        if let View::Steps { label, items, sel, done } = &back {
            let r = Resume {
                label: label.clone(),
                items: items.clone(),
                sel: *sel,
                done: done.clone(),
                verb,
            };
            if state_body(&self.pane, &self.cap.cmd, &r).is_some() {
                self.resume = Some(r);
            } else {
                eprintln!("cockpit-tui: the steps are too long to keep open");
            }
        }
        self.view = back;
        self.action = Some(Action { verb, text });
        self.exit();
    }

    fn confirm_key(&mut self, k: KeyEvent, text: String, send: bool, warn: Vec<String>, back: Box<View>, scroll: usize) {
        let verb = if send { "send" } else { "paste" };
        match k.code {
            KeyCode::Char('y') => return self.act(verb, text, *back),
            KeyCode::Enter if !needs_y(&text, send) => return self.act(verb, text, *back),
            KeyCode::Esc | KeyCode::Char('n' | 'q') => {
                self.note = None;
                return self.view = *back;
            }
            KeyCode::Enter => self.say(Level::Warn, "press y to send this: Enter does not confirm a multi-line send"),
            _ => {}
        }
        let scroll = self.scrolled(k.code, scroll).unwrap_or(scroll);
        self.view = View::Confirm {
            text,
            send,
            warn,
            back,
            scroll,
        };
    }

    fn home_key(&mut self, k: KeyEvent, sel: usize) {
        let tiles = self.tiles();
        let pick = match k.code {
            KeyCode::Esc => return self.exit(),
            KeyCode::Up | KeyCode::Char('k') => {
                return self.view = View::Home {
                    sel: sel.saturating_sub(1),
                };
            }
            KeyCode::Down | KeyCode::Char('j') => {
                return self.view = View::Home {
                    sel: (sel + 1).min(tiles.len() - 1),
                };
            }
            KeyCode::Enter => sel,
            KeyCode::Char(c @ '1'..='9') => c as usize - '1' as usize,
            _ => return,
        };
        let Some((tile, ..)) = tiles.get(pick) else { return };
        match tile {
            Tile::WritePrompt => self.compose(OutType::Prompt),
            Tile::WriteCommand => self.compose(OutType::Command),
            Tile::Compose => self.compose(OutType::Auto),
            Tile::Explain => self.view = View::ExplainAsk { sel: 0 },
            // under the tablet floor the flip would strand the user on the card
            Tile::Mode if !self.tablet && self.size.is_some_and(|(w, h)| w < TABLET_MIN_W || h < TABLET_MIN_H) => {
                self.say(
                    Level::Warn,
                    format!("the window is too small for tablet mode ({TABLET_MIN_W}x{TABLET_MIN_H})"),
                );
            }
            Tile::Mode => {
                self.tablet = !self.tablet;
                let m = if self.tablet { "tablet" } else { "pc" };
                self.say(Level::Info, format!("{m} mode until the panel closes"));
            }
            Tile::Close => self.exit(),
        }
    }

    fn compose(&mut self, out: OutType) {
        self.out = out;
        self.view = View::Compose;
    }

    fn compose_key(&mut self, k: KeyEvent, ctrl: bool) {
        self.note = None;
        match k.code {
            KeyCode::Esc => self.view = View::Home { sel: 0 },
            KeyCode::Char('s') if ctrl => self.write(),
            KeyCode::F(2) => self.write(),
            KeyCode::Char('f') if ctrl => self.fix(),
            KeyCode::Char('t') if ctrl => {
                if self.draft.trim().is_empty() {
                    self.say(Level::Warn, "nothing to insert yet");
                } else {
                    self.insert(self.draft.clone(), View::Compose);
                }
            }
            KeyCode::Char('n') if ctrl => self.model = (self.model + 1) % MODELS.len(),
            KeyCode::Char('r') if ctrl => self.toggle_explore(),
            // by row as drawn, so the box's width as `render` wraps it; before a frame, unwrapped
            KeyCode::Up | KeyCode::Down => {
                let w = self.size.map_or(usize::MAX, |(w, _)| usize::from(w).saturating_sub(4));
                self.cur = super::view::vertical(&self.draft, self.cur, w, k.code == KeyCode::Up);
            }
            KeyCode::Tab => {
                let i = OUT_CYCLE.iter().position(|o| *o == self.out).unwrap_or(0);
                self.out = OUT_CYCLE[(i + 1) % OUT_CYCLE.len()];
            }
            _ => {
                let n = edit(&mut self.draft, &mut self.cur, k, ctrl, true);
                self.cap_note(n)
            }
        }
    }

    /// ^R: explore first on or off. Refused for a window whose folder is not
    /// the one it shows (#433's rule), or that has none.
    fn toggle_explore(&mut self) {
        if self.explore {
            self.explore = false;
            return self.say(Level::Info, "explore off: the write reads the screen only");
        }
        match self.explore_dir() {
            Ok(dir) => {
                self.explore = true;
                self.say(Level::Info, format!("explore on: the write may read files under {}", dir.display()))
            }
            Err(why) => self.say(Level::Warn, format!("explore refused: {why}")),
        }
    }

    /// The folder an exploring write may read: the pane's repo, or its folder
    /// outside one, in a local window and one request.rs would start a call on;
    /// else why not, for the user.
    fn explore_dir(&self) -> Result<PathBuf, String> {
        if !self.local {
            return Err(format!(
                "{} may run on another machine, so its folder is not the one shown",
                self.cap.cmd
            ));
        }
        if self.cap.path.is_empty() {
            return Err("tmux gives no folder for this pane".into());
        }
        let root = self.explore_root.get_or_init(|| {
            let dir = toplevel(&self.cap.path, REPO_BUDGET).unwrap_or_else(|| self.cap.path.clone());
            let dir = PathBuf::from(dir);
            read_dir_ok(&dir).map(|()| dir)
        });
        root.clone()
    }

    /// A write with the toggle on, in a window local when the panel opened. Fix and Explain never read.
    pub fn explores(&self, flow: &Flow) -> bool {
        self.explore && matches!(flow, Flow::Write { .. }) && self.explore_dir().is_ok()
    }

    /// What the call for `flow` may use: [`Profile::TEXT`], or for an exploring
    /// write a read-only one on the pane's folder under the explore caps, per call;
    /// the viewer's ask reads its repo and the web under the owner's caps (#365 D7).
    pub fn profile_for(&self, flow: &Flow) -> Profile {
        use super::ask::{BUDGET_USD, TIMEOUT};
        match (flow, self.explores(flow).then(|| self.explore_dir())) {
            (Flow::Ask(a), _) => Profile::read_only(a.repo.clone(), true, BUDGET_USD, TIMEOUT),
            (_, Some(Ok(d))) => Profile::read_only(d, false, EXPLORE_BUDGET_USD, EXPLORE_TIMEOUT),
            _ => Profile::TEXT,
        }
    }

    /// [`hint`], but for an exploring call's own caps, which no setting raises: esc
    /// leads back to the draft from any view a call starts on, and ^R is there.
    fn fail_hint(&self, f: &Fail) -> Option<String> {
        let exploring = self.run.as_ref().is_some_and(|r| self.explores(&r.flow));
        if exploring && matches!(f, Fail::Timeout | Fail::Budget { .. }) {
            // the line above names the cap; this says what to do about it
            return Some("explore caps each call: r retries, or esc back to the draft, where ^R turns it off".into());
        }
        hint(f).map(String::from)
    }

    fn need_draft(&mut self) -> bool {
        let empty = self.draft.trim().is_empty();
        if empty {
            self.say(Level::Warn, "type something first");
        }
        !empty
    }

    fn write(&mut self) {
        if self.need_draft() {
            let flow = Flow::Write {
                draft: self.draft.clone(),
                out: self.out,
            };
            self.start(flow, vec![], vec![], ROUNDS_WRITE, View::Compose);
        }
    }

    fn fix(&mut self) {
        if self.need_draft() {
            self.start(Flow::Fix { draft: self.draft.clone() }, vec![], vec![], 0, View::Compose);
        }
    }

    fn explain(&mut self, question: &str, shown: &str, back: View) {
        let turns = vec![Turn {
            role: Role::User,
            text: question.into(),
        }];
        self.start(Flow::Explain, turns, vec![(true, shown.into())], ROUNDS_EXPLAIN, back);
    }

    fn start(&mut self, flow: Flow, turns: Vec<Turn>, log: Vec<(bool, String)>, max: usize, back: View) {
        let Route { model, why, .. } = route(&flow, MODELS[self.model]);
        self.run = Some(Run {
            flow,
            turns,
            rounds: 0,
            max,
            qstack: vec![],
            question: String::new(),
            options: vec![],
            log,
            ready: None,
            model,
            why,
            pending_turn: false,
            guessed_from: None,
        });
        self.busy(back);
    }

    fn busy(&mut self, back: View) {
        self.busy_back = Some(back);
        self.view = View::Busy;
        self.pending = true;
        self.note = None;
        self.hidden = self.run.as_ref().map_or(0, |r| self.envelope(r).1);
    }

    fn envelope(&self, r: &Run) -> (String, usize) {
        build_counted(&r.flow, &self.cap, &r.turns, r.max.saturating_sub(r.rounds))
    }

    /// The system prompt, the user prompt and the model for the pending call.
    pub fn request_parts(&self) -> Option<(String, String, String)> {
        self.request_noted().map(|(parts, _)| parts)
    }

    /// [`App::request_parts`], and why a prompt override was not used.
    fn request_noted(&self) -> Option<((String, String, String), Option<String>)> {
        let r = self.run.as_ref()?;
        let (system, note) = match &r.flow {
            Flow::Write { out, .. } => {
                let ctx = Ctx {
                    out: *out,
                    window: window_kind(&self.cap.name, &self.cap.cmd),
                };
                if self.explores(&r.flow) {
                    prompts::write_explore_noted(&ctx)
                } else {
                    prompts::write_noted(&ctx)
                }
            }
            Flow::Fix { .. } => prompts::fix_noted(),
            Flow::Explain => prompts::explain_noted(),
            Flow::Ask(_) => prompts::ask_code_noted(),
        };
        Some(((system, self.envelope(r).0, r.model.clone()), note))
    }

    /// Forks the pending call, if there is one.
    pub fn launch(&mut self, call: &Call) {
        if !std::mem::take(&mut self.pending) {
            return;
        }
        if let Some(((system, prompt, model), note)) = self.request_noted() {
            if let Some(n) = note {
                self.say(Level::Warn, n);
            }
            let p = self.run.as_ref().map_or(Profile::TEXT, |r| self.profile_for(&r.flow));
            self.timeout = p.timeout.unwrap_or(call.timeout);
            self.req = Some(Request::spawn_with(call, &p, &system, &prompt, &model));
            self.started = Some(Instant::now());
        }
    }

    /// The viewer's `?` (#365): a Q&A about `a`, opened on the question's box.
    /// Nothing is sent until the question is.
    pub fn ask_open(&mut self, a: super::ask::AskCode) {
        let back = View::Home { sel: 0 };
        self.start(Flow::Ask(a), vec![], vec![], 0, back.clone());
        self.pending = false;
        self.busy_back = None;
        self.view = View::Input {
            kind: InputKind::FollowUp,
            text: String::new(),
            cur: 0,
            back: Box::new(back),
        };
    }

    /// Takes a finished call's answer, if one has arrived.
    pub fn poll(&mut self) -> bool {
        let Some(a) = self.req.as_mut().and_then(Request::poll) else {
            return false;
        };
        // read before reply(), which drops the Request; Some only for a json call.
        // A failed call that read nothing has nothing to add to its error
        let read = self.req.as_ref().and_then(Request::raw).and_then(parse::files_read);
        let read = read.filter(|r| a.is_ok() || !r.is_empty());
        self.reply(a);
        self.note_reads(read);
        true
    }

    /// The files a reading call read, relative to the pane's folder (an exploring
    /// write) or the repo (an ask), on the view its answer opened: after a warning
    /// there, never in its place.
    fn note_reads(&mut self, read: Option<Vec<String>>) {
        let Some(read) = read else { return };
        let root = match self.run.as_ref().map(|r| &r.flow) {
            Some(Flow::Ask(a)) => a.repo.display().to_string(),
            _ => match self.explore_dir() {
                Ok(d) => d.display().to_string(),
                Err(_) => self.cap.path.clone(),
            },
        };
        let root = format!("{}/", root.trim_end_matches('/'));
        // the model names the files: a bidi or control character in one is not drawn
        let names: Vec<String> = read
            .iter()
            .map(|p| crate::pick::clean(p.strip_prefix(&root).unwrap_or(p)))
            .collect();
        let text = match names.len() {
            0 => "read no files".to_string(),
            1 => format!("read 1 file: {}", names[0]),
            n if n <= 3 => format!("read {n} files: {}", names.join(", ")),
            n => format!("read {n} files: {} and {} more", names[..3].join(", "), n - 3),
        };
        match self.note.take() {
            Some(Note { level, text: was }) => self.say(level, format!("{was}; {text}")),
            None => self.say(Level::Info, text),
        }
    }

    fn cancel(&mut self) {
        if let Some(mut r) = self.req.take() {
            r.cancel();
        }
        self.pending = false;
        self.drop_pending();
        self.say(Level::Info, "cancelled");
        self.view = self.busy_back.take().unwrap_or(View::Home { sel: 0 });
    }

    /// The user turn the failed or cancelled call was carrying.
    fn drop_pending(&mut self) {
        if let Some(r) = self.run.as_mut()
            && std::mem::take(&mut r.pending_turn)
        {
            r.turns.pop();
            if r.log.last().is_some_and(|(you, _)| *you) {
                r.log.pop();
            }
            if let Some(n) = r.guessed_from.take() {
                r.rounds = n;
            }
        }
    }

    fn retry(&mut self) {
        let back = self.busy_back.clone().unwrap_or(View::Home { sel: 0 });
        self.busy(back);
    }

    /// The answer from claude, or why there is none.
    pub fn reply(&mut self, a: Result<String, Fail>) {
        self.req = None;
        self.started = None;
        let out = match a {
            Ok(o) => o,
            Err(f) => {
                eprintln!("cockpit-tui: claude failed: {f}");
                let line = match f {
                    Fail::Timeout if !self.timeout.is_zero() => format!("claude took longer than {} s", self.timeout.as_secs()),
                    _ => {
                        let t = f.to_string();
                        let cut = if t.chars().count() > 160 { "..." } else { "" };
                        format!("claude failed: {}{cut}", t.chars().take(160).collect::<String>())
                    }
                };
                self.view = View::Error {
                    line,
                    hint: self.fail_hint(&f),
                };
                return;
            }
        };
        let Some(r) = self.run.as_mut() else { return };
        // what the call carried, for a write that asks with no questions left
        let carried = Carried {
            pending: std::mem::take(&mut r.pending_turn),
            from: r.guessed_from.take(),
        };
        let (rounds, max) = (r.rounds, r.max);
        let mut cut = None;
        let reply = match &r.flow {
            Flow::Write { out: want, .. } => {
                let p = parse::write(&out, *want);
                cut = p.cut_off;
                p.reply
            }
            Flow::Fix { .. } => parse::fix(&out),
            Flow::Explain => parse::explain(&out),
            Flow::Ask(_) => parse::explain(&out),
        };
        let explain = r.flow.is_qa();
        if let Some(t) = cut {
            self.say(Level::Warn, format!("the reply looks cut off (no </{t}>): check it, or r retries"));
        }
        match reply {
            Reply::Question { text, options } if rounds < max => self.ask(text, options),
            Reply::Question { text, .. } if explain => self.answer(out.trim().into(), format!("(Claude still asks: {text})"), None),
            // a question is not text to paste: no Result, so no insert or send
            Reply::Question { text, .. } => self.write_now(text, carried),
            Reply::Answer { text, ready } => self.answer(out.trim().into(), text, ready),
            Reply::Prompt(t) => self.result("Prompt for Claude Code", t),
            Reply::Text(t) if matches!(self.run.as_ref().map(|r| &r.flow), Some(Flow::Fix { .. })) => self.result("Fixed text", t),
            Reply::Text(t) => self.result("Text", t),
            Reply::Hint(t) => self.result("Hint", t),
            Reply::Format { lang, body } => {
                let lang = if lang.is_empty() { "text".into() } else { lang };
                self.result(&format!("Format ({lang})"), body)
            }
            Reply::Steps { steps, command } => {
                let lang = steps
                    .iter()
                    .find(|s| !s.cmd.is_empty())
                    .map_or("bash", |s| s.lang.as_str())
                    .to_string();
                let label = if command || steps.len() == 1 {
                    format!("Command ({lang})")
                } else {
                    format!("Steps ({lang})")
                };
                let r = Resume {
                    label,
                    done: vec![false; steps.len()],
                    items: steps,
                    sel: 0,
                    verb: "paste",
                };
                // said here, while it can be read: the panel closes on the action
                if state_body(&self.pane, &self.cap.cmd, &r).is_none() {
                    self.say(Level::Warn, "steps too long to keep open: the panel closes after one");
                }
                self.view = View::Steps {
                    label: r.label,
                    items: r.items,
                    sel: 0,
                    done: r.done,
                };
            }
            Reply::Untagged(t) => self.result("Result (unlabelled)", t),
            Reply::Unsupported(tag) => {
                eprintln!("cockpit-tui: claude answered with <{tag}>");
                self.view = View::Error {
                    line: "file edits are not supported in the panel".into(),
                    hint: Some("ask for a command or a prompt for Claude Code instead, or r retries".into()),
                };
            }
        }
    }

    /// A write that asks with no questions left is told once to write it, with
    /// Claude Code's options or a placeholder for what only the user knows. Asked
    /// again, or a fix, the error; esc then drops what the call carried.
    fn write_now(&mut self, text: String, Carried { pending, from }: Carried) {
        let Some(r) = self.run.as_mut() else { return };
        (r.pending_turn, r.guessed_from) = (pending, from);
        let write = matches!(r.flow, Flow::Write { .. });
        let last = r.turns.last_mut().filter(|t| t.role == Role::User && pending);
        if !write || last.as_ref().is_some_and(|t| t.text.ends_with(WRITE_NOW)) {
            self.view = View::Error {
                line: format!("no questions left, and Claude still asks: {text}"),
                hint: Some("esc goes back; put what it asks in your request and write again".into()),
            };
            return;
        }
        eprintln!("cockpit-tui: a write asked with no questions left, retrying once");
        // into the turn in flight, so a cancel drops the answer with it
        match last {
            Some(t) => t.text = format!("{}\n\n{WRITE_NOW}", t.text),
            None => r.turns.push(Turn {
                role: Role::User,
                text: WRITE_NOW.into(),
            }),
        }
        r.pending_turn = true;
        self.retry();
    }

    fn result(&mut self, label: &str, text: String) {
        self.view = View::Result {
            label: label.into(),
            text,
            scroll: 0,
        };
    }

    fn answer(&mut self, raw: String, text: String, ready: Option<String>) {
        if let Some(r) = self.run.as_mut() {
            r.turns.push(Turn {
                role: Role::Assistant,
                text: raw,
            });
            let shown = if text.is_empty() { "(see the prompt below)".into() } else { text };
            r.log.push((false, shown));
            if ready.is_some() {
                r.ready = ready;
            }
        }
        self.view = View::Answer {
            scroll: usize::MAX,
            armed: false,
        };
    }

    fn ask(&mut self, text: String, options: Vec<Opt>) {
        let Some(r) = self.run.as_mut() else { return };
        r.rounds += 1;
        let listed: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
        let turn = if listed.is_empty() {
            text.clone()
        } else {
            format!("{text} Options: {}", listed.join(" | "))
        };
        r.turns.push(Turn {
            role: Role::Assistant,
            text: turn,
        });
        r.qstack.push((text.clone(), options.clone(), r.turns.clone()));
        r.question = text;
        r.options = options;
        self.view = View::Question { sel: 0, scroll: None };
    }

    fn pick(&mut self, answer: String) {
        let Some(r) = self.run.as_mut() else { return };
        if r.flow.is_qa() {
            r.log.push((true, answer.clone()));
        }
        r.turns.push(Turn {
            role: Role::User,
            text: answer,
        });
        r.pending_turn = true;
        self.busy(View::Question { sel: 0, scroll: None });
    }

    /// Where [`App::leave`] goes, named for the footer and the `?` overlay.
    pub fn leave_to(&self) -> &'static str {
        let answered = self.run.as_ref().is_some_and(|r| r.log.iter().any(|(you, _)| !you));
        match self.run.as_ref().map(|r| &r.flow) {
            Some(f) if f.is_qa() && answered => "answer",
            Some(f) if f.is_qa() => "home",
            None => "home",
            Some(_) => "draft",
        }
    }

    /// Out of the question rounds, to where the flow began.
    fn leave(&mut self) {
        match self.leave_to() {
            "answer" => {
                if let Some(r) = self.run.as_mut() {
                    r.qstack.clear();
                }
                self.view = View::Answer {
                    scroll: usize::MAX,
                    armed: false,
                };
            }
            "home" => {
                self.run = None;
                self.view = View::Home { sel: 0 };
            }
            _ => {
                self.run = None;
                self.view = View::Compose;
            }
        }
    }

    fn question_key(&mut self, k: KeyEvent, sel: usize, scroll: Option<usize>) {
        let n = self.run.as_ref().map_or(0, |r| r.options.len());
        let page = self.room.saturating_sub(1).max(1);
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.view = View::Question {
                    sel: sel.saturating_sub(1),
                    scroll: None,
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.view = View::Question {
                    sel: (sel + 1).min(n.saturating_sub(1)),
                    scroll: None,
                }
            }
            // from the selection, PgUp goes to the question's start: it is what was cut
            KeyCode::PageUp => {
                let scroll = Some(scroll.map_or(0, |s| s.saturating_sub(page)));
                self.view = View::Question { sel, scroll }
            }
            // at the bottom it follows the selection again: Enter must not need a second press
            KeyCode::PageDown => {
                let scroll = scroll.map(|s| s + page).filter(|&s| s < self.bottom);
                self.view = View::Question { sel, scroll }
            }
            KeyCode::Char(c @ '1'..='4') if (c as usize - '1' as usize) < n => self.pick_option(c as usize - '1' as usize),
            // after PgUp the selection may be off screen: Enter shows it before it answers
            KeyCode::Enter if n > 0 && scroll.is_some() => self.view = View::Question { sel, scroll: None },
            KeyCode::Enter if n > 0 => self.pick_option(sel.min(n - 1)),
            KeyCode::Char('o') | KeyCode::Enter => {
                self.view = View::Input {
                    kind: InputKind::Other,
                    text: String::new(),
                    cur: 0,
                    back: Box::new(View::Question { sel, scroll: None }),
                }
            }
            KeyCode::Char('g') => {
                if let Some(r) = self.run.as_mut() {
                    r.guessed_from = Some(r.rounds);
                    r.rounds = r.max;
                }
                self.pick(BEST_GUESS.into());
            }
            KeyCode::Backspace | KeyCode::Left => self.back(),
            KeyCode::Esc => self.leave(),
            _ => {}
        }
    }

    fn pick_option(&mut self, i: usize) {
        if let Some(label) = self.run.as_ref().and_then(|r| r.options.get(i)).map(|o| o.label.clone()) {
            self.pick(label);
        }
    }

    /// The previous round with its answers, or out of the flow.
    fn back(&mut self) {
        let Some(r) = self.run.as_mut() else { return self.leave() };
        r.qstack.pop();
        match r.qstack.last().cloned() {
            Some((q, opts, turns)) => {
                r.question = q;
                r.options = opts;
                r.turns = turns;
                r.rounds = r.qstack.len();
                self.view = View::Question { sel: 0, scroll: None };
            }
            None => self.leave(),
        }
    }

    fn input_key(&mut self, k: KeyEvent, ctrl: bool, mut kind: InputKind, mut text: String, mut cur: usize, back: View) {
        match k.code {
            KeyCode::Esc => {
                self.note = None;
                return self.view = back;
            }
            KeyCode::Enter => {
                let t = text.trim().to_string();
                // an answer or a follow-up with no conversation to join: the box stays
                let joins = kind == InputKind::Explain || self.run.is_some();
                if !t.is_empty() && joins {
                    return match kind {
                        InputKind::Other => self.pick(t),
                        InputKind::Explain => self.explain(&t, &t, back),
                        InputKind::Correct(i) => self.correct(i, t, back),
                        InputKind::FollowUp => {
                            // an ask's first question: esc on its call comes back to it
                            let first_ask = self.run.as_ref().is_some_and(first_ask);
                            let back = if first_ask {
                                let (text, cur) = (t.clone(), t.len());
                                let back = Box::new(back);
                                View::Input { kind, text, cur, back }
                            } else {
                                back
                            };
                            if let Some(r) = self.run.as_mut() {
                                r.log.push((true, t.clone()));
                                r.turns.push(Turn { role: Role::User, text: t });
                                r.rounds = 0;
                                r.qstack.clear();
                                r.pending_turn = true;
                            }
                            self.busy(back);
                        }
                    };
                }
            }
            // which assumption the correction is for: the next, round to the first
            KeyCode::Tab => {
                if let (InputKind::Correct(i), View::Result { text: t, .. }) = (kind, &back) {
                    kind = InputKind::Correct((i + 1) % assumed(t).len().max(1));
                }
            }
            _ => self.cap_note(edit(&mut text, &mut cur, k, ctrl, false)),
        }
        self.view = View::Input {
            kind,
            text,
            cur,
            back: Box::new(back),
        };
    }

    fn result_key(&mut self, k: KeyEvent, text: String, scroll: usize, label: String) {
        if let Some(scroll) = self.scrolled(k.code, scroll) {
            return self.view = View::Result { label, text, scroll };
        }
        match k.code {
            KeyCode::Char('i') => {
                let back = View::Result {
                    label,
                    text: text.clone(),
                    scroll,
                };
                self.insert(text, back);
            }
            KeyCode::Char('s') => {
                let back = View::Result {
                    label,
                    text: text.clone(),
                    scroll,
                };
                self.confirm(text, true, back);
            }
            KeyCode::Char('x') => {
                self.copy_out(&text);
                self.view = View::Result { label, text, scroll };
            }
            KeyCode::Char('r') => self.retry_from(View::Result { label, text, scroll }),
            KeyCode::Char('a') if self.correctable(&text) => {
                let back = Box::new(View::Result { label, text, scroll });
                self.view = View::Input {
                    kind: InputKind::Correct(0),
                    text: String::new(),
                    cur: 0,
                    back,
                };
            }
            KeyCode::Char('c') => {
                (self.draft, self.cur) = (String::new(), 0);
                let n = put(&mut self.draft, &mut self.cur, &text);
                self.cap_note(n);
                self.run = None;
                self.view = View::Compose;
            }
            KeyCode::Esc => self.leave(),
            _ => self.view = View::Result { label, text, scroll },
        }
    }

    /// A written result with `Assumed:` lines: `a` corrects one.
    pub fn correctable(&self, text: &str) -> bool {
        let write = self.run.as_ref().is_some_and(|r| matches!(r.flow, Flow::Write { .. }));
        write && !assumed(text).is_empty()
    }

    /// One more round with the owner's correction as a turn. It takes a
    /// question from the budget, but never past it: the owner's own word is
    /// not refused at zero.
    fn correct(&mut self, i: usize, fix: String, back: View) {
        let turn = match &back {
            View::Result { text, .. } => assumed(text).get(i).map(|line| {
                let redo = "Write it again with this in place of that assumption. It was:";
                format!("Correction to \"{line}\": {fix}\n{redo}\n{text}")
            }),
            _ => None,
        };
        let (Some(turn), Some(r)) = (turn, self.run.as_mut()) else {
            return self.view = back;
        };
        r.turns.push(Turn {
            role: Role::User,
            text: turn,
        });
        r.guessed_from = Some(r.rounds);
        r.rounds = (r.rounds + 1).min(r.max);
        r.pending_turn = true;
        self.busy(back);
    }

    fn retry_from(&mut self, here: View) {
        if self.run.is_some() {
            self.busy(here);
        } else {
            self.view = here;
        }
    }

    /// What the confirm says before an insert (`send` false) or a send.
    pub fn warnings(&self, text: &str, send: bool) -> Vec<String> {
        let mut w: Vec<String> = match self.session {
            Some(Session::WaitingChoice) => vec!["⚠ Claude Code is waiting on a menu choice: typing now answers that menu".into()],
            Some(Session::Working) if send => vec!["Claude Code is still working; the prompt waits in its input".into()],
            _ => vec![],
        };
        w.extend(self.line_warning(text));
        w
    }

    fn steps_key(&mut self, k: KeyEvent, label: String, items: Vec<Step>, mut sel: usize, done: Vec<bool>) {
        let last = items.len().saturating_sub(1);
        let cmd = items.get(sel).map(|s| s.cmd.clone()).unwrap_or_default();
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => sel = sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => sel = (sel + 1).min(last),
            KeyCode::Char(c @ '1'..='9') if (c as usize - '1' as usize) < items.len() => sel = c as usize - '1' as usize,
            KeyCode::Char('x') => {
                let s = items.get(sel).map_or("", |s| if s.cmd.is_empty() { &s.note } else { &s.cmd });
                self.copy_out(s);
            }
            KeyCode::Char('i' | 's') if cmd.is_empty() => self.say(Level::Info, "this step is done outside the terminal"),
            KeyCode::Char(c @ ('i' | 's')) => {
                let here = View::Steps { label, items, sel, done };
                if c == 'i' {
                    return self.insert(cmd, here);
                }
                return self.confirm(cmd, true, here);
            }
            KeyCode::Char('r') => return self.retry_from(View::Steps { label, items, sel, done }),
            KeyCode::Esc => return self.leave(),
            _ => {}
        }
        self.view = View::Steps { label, items, sel, done };
    }

    fn explain_ask_key(&mut self, k: KeyEvent, sel: usize) {
        let pick = match k.code {
            KeyCode::Esc => return self.view = View::Home { sel: 0 },
            KeyCode::Up | KeyCode::Char('k') => {
                return self.view = View::ExplainAsk {
                    sel: sel.saturating_sub(1),
                };
            }
            KeyCode::Down | KeyCode::Char('j') => {
                return self.view = View::ExplainAsk {
                    sel: (sel + 1).min(QUICK.len()),
                };
            }
            KeyCode::Enter => sel,
            KeyCode::Char(c @ '1'..='5') => c as usize - '1' as usize,
            _ => return,
        };
        let here = View::ExplainAsk { sel };
        match QUICK.get(pick) {
            Some((shown, q)) => self.explain(q, shown, here),
            None => {
                self.view = View::Input {
                    kind: InputKind::Explain,
                    text: String::new(),
                    cur: 0,
                    back: Box::new(here),
                }
            }
        }
    }

    fn answer_key(&mut self, k: KeyEvent, scroll: usize, armed: bool) {
        let ready = self.run.as_ref().and_then(|r| r.ready.clone());
        let esc = matches!(k.code, KeyCode::Esc | KeyCode::Char('q'));
        if armed && !esc {
            self.note = None;
            self.view = View::Answer { scroll, armed: false };
        }
        if let Some(scroll) = self.scrolled(k.code, scroll) {
            return self.view = View::Answer { scroll, armed: false };
        }
        match k.code {
            KeyCode::Char('x') => {
                // the prompt is what gets pasted elsewhere; without one, the last answer
                let last = self
                    .run
                    .as_ref()
                    .and_then(|r| r.log.iter().rfind(|(you, _)| !you))
                    .map(|l| l.1.clone());
                self.copy_out(&ready.or(last).unwrap_or_default());
            }
            KeyCode::Char('f') => {
                let back = Box::new(View::Answer {
                    scroll: usize::MAX,
                    armed: false,
                });
                self.view = View::Input {
                    kind: InputKind::FollowUp,
                    text: String::new(),
                    cur: 0,
                    back,
                };
            }
            KeyCode::Char(c @ ('i' | 's')) => {
                let Some(text) = ready else {
                    return self.say(Level::Info, "no prompt for the agent in this answer");
                };
                self.confirm(text, c == 's', View::Answer { scroll, armed: false });
            }
            // the transcript is paid-for calls with no way back: one esc or q only arms
            _ if esc && !armed => {
                let again = if self.tablet {
                    "tap discard it, or esc again, to drop this conversation"
                } else {
                    "esc or q again to discard this conversation"
                };
                self.say(Level::Warn, again);
                // the note takes a body row: a view on the last line stays on it
                let scroll = if scroll >= self.bottom { usize::MAX } else { scroll };
                self.view = View::Answer { scroll, armed: true };
            }
            _ if esc => {
                self.note = None;
                self.run = None;
                self.view = View::Home { sel: 0 };
            }
            _ => {}
        }
    }
}

/// The `Assumed: <what> (<why>)` lines the result ends with, trimmed, in order.
/// Only the closing run: one inside the prompt, a quoted log say, is the prompt's.
pub fn assumed(text: &str) -> Vec<&str> {
    let mut all: Vec<&str> = text
        .lines()
        .map(str::trim)
        .rev()
        .skip_while(|l| l.is_empty())
        .take_while(|l| l.starts_with("Assumed:"))
        .collect();
    all.reverse();
    all
}

/// The shared text-box keys, at the cursor `cur`. `multi` makes Enter a
/// newline. Returns how many characters the MAX_DRAFT cap turned away.
fn edit(text: &mut String, cur: &mut usize, k: KeyEvent, ctrl: bool, multi: bool) -> usize {
    let c = boundary(text, *cur);
    let home = text[..c].rfind('\n').map_or(0, |i| i + 1);
    let end = text[c..].find('\n').map_or(text.len(), |i| c + i);
    let at = |i: usize| (i, i);
    // the cursor goes to `from`, and `from..to` is deleted
    let (from, to) = match k.code {
        KeyCode::Left if ctrl => at(word_left(text, c)),
        KeyCode::Right if ctrl => at(word_right(text, c)),
        KeyCode::Left => at(prev(text, c)),
        KeyCode::Right => at(next(text, c)),
        KeyCode::Home => at(home),
        KeyCode::Char('a') if ctrl => at(home),
        KeyCode::End => at(end),
        KeyCode::Char('e') if ctrl => at(end),
        KeyCode::Backspace => (prev(text, c), c),
        KeyCode::Delete => (c, next(text, c)),
        KeyCode::Char('w') if ctrl => (word_left(text, c), c),
        // readline's ^U: the whole text before the cursor, not its line
        KeyCode::Char('u') if ctrl => (0, c),
        KeyCode::Enter if multi => {
            *cur = c;
            return put(text, cur, "\n");
        }
        KeyCode::Char(ch) if !ctrl => {
            *cur = c;
            return put(text, cur, ch.encode_utf8(&mut [0; 4]));
        }
        _ => at(c),
    };
    text.replace_range(from..to, "");
    *cur = from;
    0
}

/// `i` moved back onto a grapheme boundary, and into the text.
pub fn boundary(text: &str, i: usize) -> usize {
    let mut i = i.min(text.len());
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    if GraphemeCursor::new(i, text.len(), true).is_boundary(text, 0) == Ok(true) {
        i
    } else {
        prev(text, i)
    }
}

/// Moves by grapheme cluster: a ZWJ family, a skin tone or a flag draws as
/// one symbol, and a cursor inside it would split it on Backspace.
fn prev(text: &str, i: usize) -> usize {
    let r = GraphemeCursor::new(i, text.len(), true).prev_boundary(text, 0);
    r.ok().flatten().unwrap_or(0)
}

fn next(text: &str, i: usize) -> usize {
    let r = GraphemeCursor::new(i, text.len(), true).next_boundary(text, 0);
    r.ok().flatten().unwrap_or(text.len())
}

/// A cluster is whitespace by its first char: a space with a mark on it too.
fn blank(g: &str) -> bool {
    g.starts_with(char::is_whitespace)
}

/// The start of the word before `i`, past the whitespace in between.
fn word_left(text: &str, i: usize) -> usize {
    let mut at = 0;
    for (j, g) in text[..i].grapheme_indices(true).rev().skip_while(|(_, g)| blank(g)) {
        if blank(g) {
            break;
        }
        at = j;
    }
    at
}

/// The end of the word after `i`, past the whitespace in between.
fn word_right(text: &str, i: usize) -> usize {
    let mut it = text[i..].grapheme_indices(true).skip_while(|(_, g)| blank(g));
    it.find(|(_, g)| blank(g)).map_or(text.len(), |(j, _)| i + j)
}

/// Inserts at the cursor the clusters that fit under MAX_DRAFT; the cursor goes
/// after them and past a mark they joined. Returns the count left out.
fn put(text: &mut String, cur: &mut usize, s: &str) -> usize {
    let mut room = MAX_DRAFT.saturating_sub(text.chars().count());
    let kept: String = s
        .graphemes(true)
        .take_while(|g| {
            let n = g.chars().count();
            let fits = n <= room;
            room = room.saturating_sub(n);
            fits
        })
        .collect();
    let c = boundary(text, *cur);
    text.insert_str(c, &kept);
    let after = c + kept.len();
    // a letter typed before a stray combining mark takes the mark: step past it
    *cur = if GraphemeCursor::new(after, text.len(), true).is_boundary(text, 0) == Ok(true) {
        after
    } else {
        next(text, after)
    };
    s.chars().count() - kept.chars().count()
}

/// A bracketed paste, cleaned like the action file. Terminals send a pasted
/// newline as CR, so a lone `\r` is a line break too.
fn pasted(s: &str, multi: bool) -> String {
    let t = sanitize(&s.replace("\r\n", "\n").replace('\r', "\n"));
    if multi { t } else { t.split('\n').collect::<Vec<_>>().join(" ") }
}

pub fn bracketed_ok(cmd: &str) -> bool {
    BRACKETED_OK.contains(&cmd) || SSH_LIKE.contains(&cmd)
}

/// What to do about a failure the panel knows; claude's own words stay on the line above.
pub fn hint(f: &Fail) -> Option<&'static str> {
    let t = match f {
        Fail::Timeout => return Some("r retries; COCKPIT_PANEL_TIMEOUT=<seconds> gives it longer"),
        Fail::Budget { .. } => return None,
        Fail::Failed(t) => t.to_ascii_lowercase(),
    };
    let has = |ws: &[&str]| ws.iter().any(|w| t.contains(w));
    if has(&["not logged in", "/login", "invalid api key", "authentication", "oauth"]) {
        Some("log in: run claude in a terminal and use /login, then r retries")
    } else if has(&["rate limit", "rate_limit", "usage limit", "429", "overloaded"]) {
        Some("a usage or rate limit: wait a minute, then r retries")
    } else if !t.starts_with("cannot write") && t.contains("(os error 2)") {
        Some("claude is not installed here: install Claude Code, or set COCKPIT_CLAUDE to its path")
    } else {
        None
    }
}

/// An ask not answered yet: esc on its call goes back to the question.
fn first_ask(r: &Run) -> bool {
    matches!(r.flow, Flow::Ask(_)) && r.log.iter().all(|(you, _)| *you)
}

/// A send of more than one line wants a `y`: a pasted block runs line by line
/// as it arrives, and an Enter held over from the last view must not fire it.
pub fn needs_y(text: &str, send: bool) -> bool {
    send && text.trim_end_matches('\n').contains('\n')
}

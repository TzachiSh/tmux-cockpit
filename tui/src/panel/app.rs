//! The panel's state machine: views, keys, and the request round trip. No
//! terminal here, so every transition is a unit test away.
use super::capture::{Capture, SSH_LIKE, Session, session_state, window_kind};
use super::envelope::{Flow, OutType, Role, Turn, build_counted};
use super::parse::{self, Opt, Reply, Step};
use super::prompts::{self, Ctx};
use super::redact::redact;
use super::request::{Call, Fail, Request};
use super::route::{Route, route};
use super::{boundary_at, boundary_before, ci};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthChar;

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
const ROUNDS_WRITE: usize = 3;
const ROUNDS_EXPLAIN: usize = 6;
const BEST_GUESS: &str = "(no preference - use your best guess and state the assumption)";

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

const STATE_HEAD: &str = "cockpit-panel-state 1";
/// Far over any real reply: write.txt asks for a handful of steps.
const STATE_MAX: u64 = 64 * 1024;
const STATE_STEPS: usize = 64;

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
    /// The rounds before a best guess, so a cancelled guess can be undone.
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
        App {
            pane: pane.into(),
            session: session_state(&cap.screen, &cap.name, &cap.cmd),
            cap,
            view: View::Home { sel: 0 },
            draft: String::new(),
            cur: 0,
            out: OutType::Auto,
            model: 0,
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
            reopened: false,
            quit: false,
            last_step: None,
            room: 0,
            bottom: 0,
            help: None,
            tiny: false,
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

    /// The wheel is Up and Down, except in a text box, where those keys do nothing
    /// but drop the note.
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
        if let Some(top) = self.help.take() {
            self.help = match k.code {
                KeyCode::Up | KeyCode::Char('k') => Some(top.saturating_sub(1)),
                KeyCode::Down | KeyCode::Char('j') => Some((top + 1).min(self.bottom)),
                _ => None,
            };
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
        if let View::Question { scroll: Some(s), .. } = &mut self.view {
            *s = (*s).min(self.bottom);
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
        let scroll = scroll.min(self.bottom);
        if self.room == 0 {
            // no body rows on screen: an offset that moved would show nothing
            return matches!(
                code,
                KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown | KeyCode::Char('j' | 'k')
            )
            .then_some(scroll);
        }
        let page = self.room.saturating_sub(1).max(1);
        let s = match code {
            KeyCode::Up | KeyCode::Char('k') => scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => scroll + 1,
            KeyCode::PageUp => scroll.saturating_sub(page),
            KeyCode::PageDown => scroll + page,
            _ => return None,
        };
        Some(s.min(self.bottom))
    }

    /// Out with no action, the call killed: a hangup, or a loop that broke.
    pub fn stop(&mut self) {
        self.action = None;
        self.resume = None;
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
            KeyCode::Enter => self.say(
                Level::Warn,
                "press y to send this: Enter does not confirm a risky or multi-line send",
            ),
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
            Flow::Write { out, .. } => prompts::write_noted(&Ctx {
                out: *out,
                window: window_kind(&self.cap.name, &self.cap.cmd),
            }),
            Flow::Fix { .. } => prompts::fix_noted(),
            Flow::Explain => prompts::explain_noted(),
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
            self.timeout = call.timeout;
            self.req = Some(Request::spawn(call, &system, &prompt, &model));
            self.started = Some(Instant::now());
        }
    }

    /// Takes a finished call's answer, if one has arrived.
    pub fn poll(&mut self) -> bool {
        let Some(a) = self.req.as_mut().and_then(Request::poll) else {
            return false;
        };
        self.reply(a);
        true
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
                    hint: hint(&f).map(String::from),
                };
                return;
            }
        };
        let Some(r) = self.run.as_mut() else { return };
        r.pending_turn = false;
        r.guessed_from = None;
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
        };
        let explain = matches!(r.flow, Flow::Explain);
        if let Some(t) = cut {
            self.say(Level::Warn, format!("the reply looks cut off (no </{t}>): check it, or r retries"));
        }
        match reply {
            Reply::Question { text, options } if rounds < max => self.ask(text, options),
            Reply::Question { text, .. } if explain => self.answer(out.trim().into(), format!("(Claude still asks: {text})"), None),
            // a question is not text to paste: no Result, so no insert or send
            Reply::Question { text, .. } => {
                self.view = View::Error {
                    line: format!("no questions left, and Claude still asks: {text}"),
                    hint: Some("esc goes back: add what it asks to the draft and write again".into()),
                };
            }
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
        if matches!(r.flow, Flow::Explain) {
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
            Some(Flow::Explain) if answered => "answer",
            Some(Flow::Explain) | None => "home",
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
            KeyCode::PageDown => {
                let scroll = scroll.map(|s| (s + page).min(self.bottom));
                self.view = View::Question { sel, scroll }
            }
            KeyCode::Char(c @ '1'..='4') if (c as usize - '1' as usize) < n => self.pick_option(c as usize - '1' as usize),
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

    fn input_key(&mut self, k: KeyEvent, ctrl: bool, kind: InputKind, mut text: String, mut cur: usize, back: View) {
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
                        InputKind::FollowUp => {
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
        if let Some(r) = risky(text) {
            w.push(format!("⚠ risky: {r}"));
        }
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

/// `i` moved back onto a char boundary, and into the text.
pub fn boundary(text: &str, i: usize) -> usize {
    let mut i = i.min(text.len());
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// A combining mark, VS16 or ZWJ: the cursor never stops before one, or it
/// would split what the terminal draws as one cell or two.
fn joins(c: char) -> bool {
    !c.is_control() && c.width() == Some(0)
}

fn prev(text: &str, i: usize) -> usize {
    text[..i].char_indices().rev().find(|&(_, c)| !joins(c)).map_or(0, |(j, _)| j)
}

fn next(text: &str, i: usize) -> usize {
    let mut it = text[i..].char_indices().skip(1);
    it.find(|&(_, c)| !joins(c)).map_or(text.len(), |(j, _)| i + j)
}

/// The start of the word before `i`, past the whitespace in between: U+00A0
/// and U+3000 are more than one byte, so offsets come from char_indices.
fn word_left(text: &str, i: usize) -> usize {
    let t = text[..i].trim_end_matches(char::is_whitespace).len();
    text[..t]
        .char_indices()
        .rfind(|(_, c)| c.is_whitespace())
        .map_or(0, |(j, c)| j + c.len_utf8())
}

/// The end of the word after `i`, past the whitespace in between.
fn word_right(text: &str, i: usize) -> usize {
    let rest = &text[i..];
    let w = rest.len() - rest.trim_start_matches(char::is_whitespace).len();
    rest[w..].find(char::is_whitespace).map_or(text.len(), |j| i + w + j)
}

/// Inserts at the cursor what fits under MAX_DRAFT, the cursor after it;
/// returns the count left out.
fn put(text: &mut String, cur: &mut usize, s: &str) -> usize {
    let room = MAX_DRAFT.saturating_sub(text.chars().count());
    let kept: String = s.chars().take(room).collect();
    let c = boundary(text, *cur);
    text.insert_str(c, &kept);
    *cur = c + kept.len();
    s.chars().count().saturating_sub(room)
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

/// A send that runs something risky, or more than one line, wants a `y`: an
/// Enter held over from the last view must not fire it.
pub fn needs_y(text: &str, send: bool) -> bool {
    send && (risky(text).is_some() || text.trim_end_matches('\n').contains('\n'))
}

/// Control characters out, `\t` and `\n` kept, `\r\n` folded: a stray ESC or
/// C1 byte pasted into a pane is a keystroke, not text.
pub fn sanitize(s: &str) -> String {
    s.replace("\r\n", "\n")
        .chars()
        .filter(|&c| c == '\t' || c == '\n' || !c.is_control())
        .collect()
}

/// `<path>.text` first, then the action line, so the wrapper never reads a
/// line whose text is not there yet. Both are created fresh, never followed.
/// `<path>.cmd` is the command the pane ran when it was captured, for the
/// wrapper to check it still runs before the paste lands.
pub fn write_action(path: &Path, pane: &str, cmd: &str, a: &Action) -> std::io::Result<()> {
    fresh(Path::new(&format!("{}.text", path.display())), &sanitize(&a.text))?;
    fresh(
        Path::new(&format!("{}.cmd", path.display())),
        &format!("{}\n", sanitize(cmd).replace('\n', " ")),
    )?;
    fresh(path, &format!("{} {pane}\n", a.verb))
}

fn fresh(p: &Path, body: &str) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .mode(0o600)
        .open(p)?;
    f.write_all(body.as_bytes())
}

fn state_esc(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn state_unesc(s: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            _ => return Err("a bad escape".into()),
        }
    }
    Ok(out)
}

/// `<path>.state`, for the wrapper to hand back with `--resume`: written before
/// the action line, created fresh and never followed, like the other two.
pub fn write_state(path: &Path, pane: &str, cmd: &str, r: &Resume) -> std::io::Result<()> {
    let b = state_body(pane, cmd, r).ok_or(std::io::Error::new(std::io::ErrorKind::InvalidInput, "steps too long to keep"))?;
    fresh(Path::new(&format!("{}.state", path.display())), &b)
}

/// The state file's text, or `None` when [`read_state`] would refuse it.
pub fn state_body(pane: &str, cmd: &str, r: &Resume) -> Option<String> {
    if r.items.len() > STATE_STEPS {
        return None;
    }
    let mut b = format!(
        "{STATE_HEAD}\npane {pane}\ncmd {}\nlabel {}\nsel {}\nverb {}\n",
        state_esc(cmd),
        state_esc(&r.label),
        r.sel,
        r.verb
    );
    for (s, d) in r.items.iter().zip(&r.done) {
        let f = [&s.lang, &s.note, &s.cmd].map(|x| state_esc(x));
        b.push_str(&format!("step {}\t{}\t{}\t{}\n", u8::from(*d), f[0], f[1], f[2]));
    }
    (b.len() as u64 <= STATE_MAX).then_some(b)
}

/// The command and the steps a [`write_state`] file holds, refused unless it is
/// a small regular file of this version, for this very pane.
pub fn read_state(path: &Path, pane: &str) -> Result<(String, Resume), String> {
    use std::io::Read;
    let small = |m: std::fs::Metadata| m.is_file() && m.len() <= STATE_MAX;
    // checked before the open, which a FIFO would block, and again on the fd itself
    if !std::fs::symlink_metadata(path).is_ok_and(small) {
        return Err("not a small regular file".into());
    }
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| e.to_string())?;
    if !f.metadata().is_ok_and(small) {
        return Err("not a small regular file".into());
    }
    let mut body = String::new();
    f.take(STATE_MAX).read_to_string(&mut body).map_err(|e| e.to_string())?;
    let Some(body) = body.strip_suffix('\n') else {
        return Err("cut short".into());
    };
    let mut lines = body.split('\n');
    if lines.next() != Some(STATE_HEAD) {
        return Err("not a version 1 state file".into());
    }
    let mut field = |key: &str| -> Result<String, String> {
        let l = lines.next().ok_or(format!("no {key} line"))?;
        let v = l
            .strip_prefix(key)
            .and_then(|v| v.strip_prefix(' '))
            .ok_or(format!("{key} expected"))?;
        state_unesc(v)
    };
    if field("pane")? != pane {
        return Err(format!("it is for another pane, not {pane}"));
    }
    let cmd = field("cmd")?;
    let label = field("label")?;
    let sel: usize = field("sel")?.parse().map_err(|_| "a bad sel".to_string())?;
    let verb = match field("verb")?.as_str() {
        "paste" => "paste",
        "send" => "send",
        _ => return Err("a bad verb".into()),
    };
    let (mut items, mut done) = (vec![], vec![]);
    for l in lines {
        let rest = l.strip_prefix("step ").ok_or("a line that is not a step")?;
        let f: Vec<&str> = rest.split('\t').collect();
        let [d, lang, note, cmd] = f[..] else {
            return Err("a step needs four fields".into());
        };
        done.push(match d {
            "0" => false,
            "1" => true,
            _ => return Err("a bad done mark".into()),
        });
        items.push(Step {
            lang: state_unesc(lang)?,
            note: state_unesc(note)?,
            cmd: state_unesc(cmd)?,
        });
    }
    if items.is_empty() || items.len() > STATE_STEPS || sel >= items.len() {
        return Err("no step to come back to".into());
    }
    Ok((
        cmd,
        Resume {
            label,
            items,
            sel,
            done,
            verb,
        },
    ))
}

fn ws(s: &str, i: usize, min: usize) -> Option<usize> {
    let n: usize = s[i..]
        .chars()
        .take_while(|c| c.is_whitespace() && *c != '\n')
        .map(char::len_utf8)
        .sum();
    (s[i..i + n].chars().count() >= min).then_some(i + n)
}

/// `word` with a word boundary after it.
fn word(s: &str, i: usize, lit: &str) -> Option<usize> {
    ci(s, i, lit).filter(|&e| boundary_at(s, e))
}

/// `a ws+ b ws+ c`, the shape of `git push`, `chmod -R` and kin.
fn seq(s: &str, i: usize, parts: &[&str]) -> Option<usize> {
    let mut j = i;
    for (k, p) in parts.iter().enumerate() {
        if k > 0 {
            j = ws(s, j, 1)?;
        }
        j = ci(s, j, p)?;
    }
    Some(j)
}

fn line_end(s: &str, i: usize) -> usize {
    s[i..].find('\n').map_or(s.len(), |n| i + n)
}

fn risky_at(s: &str, i: usize) -> Option<usize> {
    if let Some(e) = ci(s, i, ">").and_then(|j| ws(s, j, 0)).and_then(|j| ci(s, j, "/dev/sd")) {
        return Some(e);
    }
    if !boundary_before(s, i) {
        return None;
    }
    let eol = line_end(s, i);
    let simple = [
        ci(s, i, "rm").and_then(|j| ws(s, j, 1).filter(|&e| e > j)),
        word(s, i, "sudo"),
        ci(s, i, "dd").and_then(|j| ws(s, j, 1)),
        ci(s, i, "mkfs"),
        word(s, i, "shred"),
        seq(s, i, &["chmod", "-r"]),
        seq(s, i, &["chown", "-r"]),
        seq(s, i, &["kill", "-9"]),
        word(s, i, "pkill"),
        word(s, i, "killall"),
        word(s, i, "reboot"),
        seq(s, i, &["git", "reset", "--hard"]),
        seq(s, i, &["git", "clean", "-"]),
        seq(s, i, &["drop", "table"]),
    ];
    if let Some(e) = simple.into_iter().flatten().next() {
        return Some(e);
    }
    if let Some(j) = seq(s, i, &["git", "push"]).and_then(|j| ws(s, j, 1)) {
        let rest = &s[j..eol];
        let force = rest.match_indices("--force").map(|(k, m)| j + k + m.len()).chain(
            rest.match_indices("-f")
                .map(|(k, m)| j + k + m.len())
                .filter(|&e| boundary_at(s, e)),
        );
        if let Some(e) = force.max() {
            return Some(e);
        }
    }
    let fetch = word(s, i, "curl").or_else(|| word(s, i, "wget"))?;
    s[fetch..eol].match_indices('|').rev().find_map(|(k, _)| {
        let j = ws(s, fetch + k + 1, 0)?;
        let j = ci(s, j, "ba").or_else(|| ci(s, j, "z")).unwrap_or(j);
        word(s, j, "sh")
    })
}

/// The first risky fragment of a command, as type-panel flags them.
pub fn risky(cmd: &str) -> Option<String> {
    let mut i = 0;
    while let Some(c) = cmd[i..].chars().next() {
        if let Some(e) = risky_at(cmd, i) {
            return Some(cmd[i..e].trim().to_string());
        }
        i += c.len_utf8();
    }
    None
}

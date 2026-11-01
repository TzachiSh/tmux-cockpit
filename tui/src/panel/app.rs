//! The panel's state machine: views, keys, and the request round trip. No
//! terminal here, so every transition is a unit test away.
use super::capture::{Capture, Session, session_state, window_kind};
use super::envelope::{Flow, OutType, Role, Turn, build};
use super::parse::{self, Opt, Reply, Step};
use super::prompts::{self, Ctx};
use super::request::{Call, Fail, Request};
use super::route::{Route, route};
use super::{boundary_at, boundary_before};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::Instant;

pub const MODELS: [&str; 4] = ["auto", "haiku", "sonnet", "opus"];
pub const OUT_CYCLE: [OutType; 6] = [
    OutType::Auto,
    OutType::Prompt,
    OutType::Command,
    OutType::Steps,
    OutType::Text,
    OutType::Hint,
];
pub const MAX_DRAFT: usize = 8000;
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

/// Programs that take a bracketed paste as one block; anywhere else each line
/// of a multi-line paste may run as its own command.
const BRACKETED_OK: [&str; 11] = [
    "bash", "zsh", "fish", "claude", "node", "ssh", "coder", "proot", "tmux", "vim", "nvim",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tile {
    WritePrompt,
    WriteCommand,
    Compose,
    Explain,
    Close,
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
    },
    Input {
        kind: InputKind,
        text: String,
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
    },
    ExplainAsk {
        sel: usize,
    },
    Answer {
        scroll: usize,
    },
    Confirm {
        text: String,
        send: bool,
        warn: Vec<String>,
        back: Box<View>,
    },
    Error {
        line: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    /// `paste` inserts; `send` inserts and presses Enter.
    pub verb: &'static str,
    pub text: String,
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
    pub route: Route,
    /// The last turn and log entry were added for the call in flight.
    pub pending_turn: bool,
}

pub struct App {
    pub pane: String,
    pub cap: Capture,
    pub session: Option<Session>,
    pub view: View,
    pub draft: String,
    pub out: OutType,
    pub model: usize,
    pub run: Option<Run>,
    /// Set by a transition, cleared by [`App::launch`], which owns the fork.
    pub pending: bool,
    pub req: Option<Request>,
    pub started: Option<Instant>,
    pub busy_back: Option<View>,
    pub note: Option<String>,
    pub action: Option<Action>,
    pub quit: bool,
}

impl App {
    pub fn new(pane: &str, cap: Capture) -> App {
        App {
            pane: pane.into(),
            session: session_state(&cap.screen, &cap.name, &cap.cmd),
            cap,
            view: View::Home { sel: 0 },
            draft: String::new(),
            out: OutType::Auto,
            model: 0,
            run: None,
            pending: false,
            req: None,
            started: None,
            busy_back: None,
            note: None,
            action: None,
            quit: false,
        }
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
        let mut t: Vec<_> = star.into_iter().collect();
        t.extend([
            COMPOSE,
            (Tile::Explain, "Explain", "what is on this screen, and what next"),
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
        (n > 1 && !BRACKETED_OK.contains(&self.cap.cmd.as_str())).then(|| format!("⚠ {n} lines – '{}' may run each line", self.cap.cmd))
    }

    pub fn key(&mut self, k: KeyEvent) {
        if k.kind != KeyEventKind::Press {
            return;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && k.code == KeyCode::Char('c') {
            return self.exit();
        }
        let typing = matches!(self.view, View::Compose | View::Input { .. });
        if !typing && k.code == KeyCode::Char('q') {
            return self.exit();
        }
        match self.view.clone() {
            View::Home { sel } => self.home_key(k, sel),
            View::Compose => self.compose_key(k, ctrl),
            View::Busy => {
                if k.code == KeyCode::Esc {
                    self.cancel();
                }
            }
            View::Question { sel } => self.question_key(k, sel),
            View::Input { kind, text, back } => self.input_key(k, ctrl, kind, text, *back),
            View::Result { text, scroll, label } => self.result_key(k, text, scroll, label),
            View::Steps { label, items, sel } => self.steps_key(k, label, items, sel),
            View::ExplainAsk { sel } => self.explain_ask_key(k, sel),
            View::Answer { scroll } => self.answer_key(k, scroll),
            View::Confirm { text, send, back, .. } => match k.code {
                KeyCode::Enter | KeyCode::Char('y') => self.act(if send { "send" } else { "paste" }, text),
                KeyCode::Esc | KeyCode::Char('n') => self.view = *back,
                _ => {}
            },
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

    fn exit(&mut self) {
        if let Some(mut r) = self.req.take() {
            r.cancel();
        }
        self.quit = true;
    }

    /// A paste is only bracketed when the program asked for it, so a multi-line
    /// insert into anything else can run line one: that goes through a confirm.
    fn insert(&mut self, text: String, back: View) {
        if self.line_warning(&text).is_some() {
            let warn = self.warnings(&text);
            self.view = View::Confirm {
                text,
                send: false,
                warn,
                back: Box::new(back),
            };
        } else {
            self.act("paste", text);
        }
    }

    fn act(&mut self, verb: &'static str, text: String) {
        self.action = Some(Action { verb, text });
        self.exit();
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
                    self.note = Some("nothing to insert yet".into());
                } else {
                    self.insert(self.draft.clone(), View::Compose);
                }
            }
            KeyCode::Char('n') if ctrl => self.model = (self.model + 1) % MODELS.len(),
            KeyCode::Tab => {
                let i = OUT_CYCLE.iter().position(|o| *o == self.out).unwrap_or(0);
                self.out = OUT_CYCLE[(i + 1) % OUT_CYCLE.len()];
            }
            _ => edit(&mut self.draft, k, ctrl, true),
        }
    }

    fn need_draft(&mut self) -> bool {
        let empty = self.draft.trim().is_empty();
        if empty {
            self.note = Some("type something first".into());
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
        let route = route(&flow, MODELS[self.model]);
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
            route,
            pending_turn: false,
        });
        self.busy(back);
    }

    fn busy(&mut self, back: View) {
        self.busy_back = Some(back);
        self.view = View::Busy;
        self.pending = true;
        self.note = None;
    }

    /// The system prompt, the user prompt and the model for the pending call.
    pub fn request_parts(&self) -> Option<(String, String, String)> {
        let r = self.run.as_ref()?;
        let system = match &r.flow {
            Flow::Write { out, .. } => prompts::write(&Ctx {
                out: *out,
                guess: OutType::Auto,
                window: window_kind(&self.cap.name, &self.cap.cmd),
            }),
            Flow::Fix { .. } => prompts::fix(),
            Flow::Explain => prompts::explain(),
        };
        let prompt = build(&r.flow, &self.cap, &r.turns, r.max.saturating_sub(r.rounds));
        Some((system, prompt, r.route.model.clone()))
    }

    /// Forks the pending call, if there is one.
    pub fn launch(&mut self, call: &Call) {
        if !std::mem::take(&mut self.pending) {
            return;
        }
        if let Some((system, prompt, model)) = self.request_parts() {
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
        self.note = Some("cancelled".into());
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
                let line: String = f.to_string().chars().take(80).collect();
                self.view = View::Error { line };
                return;
            }
        };
        let Some(r) = self.run.as_mut() else { return };
        r.pending_turn = false;
        let (rounds, max) = (r.rounds, r.max);
        let reply = match &r.flow {
            Flow::Write { out: want, .. } => {
                let p = parse::write(&out, *want);
                if let Some(t) = p.cut_off {
                    self.note = Some(format!("the reply looks cut off (no </{t}>): check it, or r retries"));
                }
                p.reply
            }
            Flow::Fix { .. } => parse::fix(&out),
            Flow::Explain => parse::explain(&out),
        };
        let explain = matches!(r.flow, Flow::Explain);
        match reply {
            Reply::Question { text, options } if rounds < max => self.ask(text, options),
            Reply::Question { text, .. } if explain => self.answer(out.trim().into(), format!("(Claude still asks: {text})"), None),
            Reply::Question { text, .. } => self.result("Result (unlabelled)", format!("(Claude still asks: {text})")),
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
                self.view = View::Steps {
                    label,
                    items: steps,
                    sel: 0,
                };
            }
            Reply::Untagged(t) => self.result("Result (unlabelled)", t),
            Reply::Unsupported(tag) => {
                self.view = View::Error {
                    line: format!("claude answered with <{tag}>: file edits are not supported yet"),
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
        self.view = View::Answer { scroll: usize::MAX };
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
        self.view = View::Question { sel: 0 };
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
        self.busy(View::Question { sel: 0 });
    }

    /// Out of the question rounds, to where the flow began.
    fn leave(&mut self) {
        let answered = self.run.as_ref().is_some_and(|r| r.log.iter().any(|(you, _)| !you));
        match self.run.as_ref().map(|r| &r.flow) {
            Some(Flow::Explain) if answered => {
                if let Some(r) = self.run.as_mut() {
                    r.qstack.clear();
                }
                self.view = View::Answer { scroll: usize::MAX };
            }
            Some(Flow::Explain) | None => {
                self.run = None;
                self.view = View::Home { sel: 0 };
            }
            Some(_) => {
                self.run = None;
                self.view = View::Compose;
            }
        }
    }

    fn question_key(&mut self, k: KeyEvent, sel: usize) {
        let n = self.run.as_ref().map_or(0, |r| r.options.len());
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.view = View::Question {
                    sel: sel.saturating_sub(1),
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.view = View::Question {
                    sel: (sel + 1).min(n.saturating_sub(1)),
                }
            }
            KeyCode::Char(c @ '1'..='4') if (c as usize - '1' as usize) < n => self.pick_option(c as usize - '1' as usize),
            KeyCode::Enter if n > 0 => self.pick_option(sel.min(n - 1)),
            KeyCode::Char('o') | KeyCode::Enter => {
                self.view = View::Input {
                    kind: InputKind::Other,
                    text: String::new(),
                    back: Box::new(View::Question { sel }),
                }
            }
            KeyCode::Char('g') => {
                if let Some(r) = self.run.as_mut() {
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
                self.view = View::Question { sel: 0 };
            }
            None => self.leave(),
        }
    }

    fn input_key(&mut self, k: KeyEvent, ctrl: bool, kind: InputKind, mut text: String, back: View) {
        match k.code {
            KeyCode::Esc => return self.view = back,
            KeyCode::Enter => {
                let t = text.trim().to_string();
                if t.is_empty() {
                    return;
                }
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
            _ => edit(&mut text, k, ctrl, false),
        }
        self.view = View::Input {
            kind,
            text,
            back: Box::new(back),
        };
    }

    fn result_key(&mut self, k: KeyEvent, text: String, scroll: usize, label: String) {
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.view = View::Result {
                    label,
                    text,
                    scroll: scroll.saturating_sub(1),
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.view = View::Result {
                    label,
                    text,
                    scroll: scroll + 1,
                }
            }
            KeyCode::PageUp => {
                self.view = View::Result {
                    label,
                    text,
                    scroll: scroll.saturating_sub(10),
                }
            }
            KeyCode::PageDown => {
                self.view = View::Result {
                    label,
                    text,
                    scroll: scroll + 10,
                }
            }
            KeyCode::Char('i') => {
                let back = View::Result {
                    label,
                    text: text.clone(),
                    scroll,
                };
                self.insert(text, back);
            }
            KeyCode::Char('s') => {
                let warn = self.warnings(&text);
                let back = Box::new(View::Result {
                    label,
                    text: text.clone(),
                    scroll,
                });
                self.view = View::Confirm {
                    text,
                    send: true,
                    warn,
                    back,
                };
            }
            KeyCode::Char('r') => self.retry_from(View::Result { label, text, scroll }),
            KeyCode::Char('c') => {
                self.draft = text.chars().take(MAX_DRAFT).collect();
                self.run = None;
                self.view = View::Compose;
            }
            KeyCode::Esc => self.leave(),
            _ => {}
        }
    }

    fn retry_from(&mut self, here: View) {
        if self.run.is_some() {
            self.busy(here);
        }
    }

    pub fn warnings(&self, text: &str) -> Vec<String> {
        let mut w: Vec<String> = self.line_warning(text).into_iter().collect();
        if let Some(r) = risky(text) {
            w.push(format!("⚠ risky: {r}"));
        }
        w
    }

    fn steps_key(&mut self, k: KeyEvent, label: String, items: Vec<Step>, sel: usize) {
        let last = items.len().saturating_sub(1);
        let here = |sel| View::Steps {
            label: label.clone(),
            items: items.clone(),
            sel,
        };
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => self.view = here(sel.saturating_sub(1)),
            KeyCode::Down | KeyCode::Char('j') => self.view = here((sel + 1).min(last)),
            KeyCode::Char(c @ ('i' | 's')) => {
                let cmd = items.get(sel).map(|s| s.cmd.clone()).unwrap_or_default();
                if cmd.is_empty() {
                    self.note = Some("this step is done outside the terminal".into());
                } else if c == 'i' {
                    self.insert(cmd, here(sel));
                } else {
                    let warn = self.warnings(&cmd);
                    self.view = View::Confirm {
                        text: cmd,
                        send: true,
                        warn,
                        back: Box::new(here(sel)),
                    };
                }
            }
            KeyCode::Char('r') => self.retry_from(here(sel)),
            KeyCode::Esc => self.leave(),
            _ => {}
        }
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
                    back: Box::new(here),
                }
            }
        }
    }

    fn answer_key(&mut self, k: KeyEvent, scroll: usize) {
        let ready = self.run.as_ref().and_then(|r| r.ready.clone());
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.view = View::Answer {
                    scroll: scroll.saturating_sub(1),
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.view = View::Answer {
                    scroll: scroll.saturating_add(1),
                }
            }
            KeyCode::Char('f') => {
                let back = Box::new(View::Answer { scroll: usize::MAX });
                self.view = View::Input {
                    kind: InputKind::FollowUp,
                    text: String::new(),
                    back,
                };
            }
            KeyCode::Char(c @ ('i' | 's')) => {
                let Some(text) = ready else {
                    return self.note = Some("no prompt for the agent in this answer".into());
                };
                let mut warn = Vec::new();
                match self.session {
                    Some(Session::WaitingChoice) => {
                        warn.push("⚠ Claude Code is waiting on a menu choice: typing now answers that menu".into())
                    }
                    Some(Session::Working) if c == 's' => warn.push("Claude Code is still working; the prompt waits in its input".into()),
                    _ => {}
                }
                self.view = View::Confirm {
                    text,
                    send: c == 's',
                    warn,
                    back: Box::new(View::Answer { scroll }),
                };
            }
            KeyCode::Esc => {
                self.run = None;
                self.view = View::Home { sel: 0 };
            }
            _ => {}
        }
    }
}

/// The shared text-box keys. `multi` makes Enter a newline.
fn edit(text: &mut String, k: KeyEvent, ctrl: bool, multi: bool) {
    match k.code {
        KeyCode::Backspace => {
            text.pop();
        }
        KeyCode::Char('w') if ctrl => {
            let t = text.trim_end_matches(char::is_whitespace).len();
            // Past the whole separator: U+00A0 and U+3000 are more than one byte.
            let w = text[..t]
                .char_indices()
                .rfind(|(_, c)| c.is_whitespace())
                .map_or(0, |(i, c)| i + c.len_utf8());
            text.truncate(w);
        }
        KeyCode::Char('u') if ctrl => text.clear(),
        KeyCode::Enter if multi => push(text, '\n'),
        KeyCode::Char(c) if !ctrl => push(text, c),
        _ => {}
    }
}

fn push(text: &mut String, c: char) {
    if text.chars().count() < MAX_DRAFT {
        text.push(c);
    }
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
    let fresh = |p: &Path, body: &str| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(libc::O_NOFOLLOW)
            .mode(0o600)
            .open(p)?;
        f.write_all(body.as_bytes())
    };
    fresh(Path::new(&format!("{}.text", path.display())), &sanitize(&a.text))?;
    fresh(
        Path::new(&format!("{}.cmd", path.display())),
        &format!("{}\n", sanitize(cmd).replace('\n', " ")),
    )?;
    fresh(path, &format!("{} {pane}\n", a.verb))
}

fn ci(s: &str, i: usize, lit: &str) -> Option<usize> {
    let b = s.as_bytes().get(i..i + lit.len())?;
    b.eq_ignore_ascii_case(lit.as_bytes()).then_some(i + lit.len())
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

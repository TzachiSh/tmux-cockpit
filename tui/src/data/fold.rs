//! The board's transcript fold (#492): what `parse` reads from the rows, each fact
//! kept with the byte offset of the row that last set it. A window that starts at
//! `ws` holds the rows that start at or after `ws`, so `view(ws)` answers for that
//! window from a fold over a longer stretch: a fact whose row lies before `ws` is
//! one the window never saw.

use super::{Facts, State, first_line, rfc3339, transcript};
use serde_json::Value;
use std::time::SystemTime;

/// Ends the turn without closing anything: neither a prompt nor an answer.
/// It arrives as a `text` block in the array form.
const INTERRUPT: &str = "[Request interrupted by user";

/// A value and the offset of the row that set it.
type At<T> = Option<(u64, T)>;

#[derive(Clone, Default)]
pub(super) struct Fold {
    tool: At<String>,
    text: At<String>,
    user: At<(String, Option<SystemTime>)>,
    /// the newest assistant row with a timestamp
    at: At<SystemTime>,
    /// the newest row of any type with a timestamp
    newest: At<SystemTime>,
    cwd: At<String>,
    turn_over: At<bool>,
    /// the newest user or assistant row
    saw: Option<u64>,
    /// An AskUserQuestion is open until a user row carries its tool_result:
    /// answers, dismissals and denials write one, an interrupt does not.
    asks: Vec<(u64, String)>,
    /// #453: the newest tool_use's id and, for an AskUserQuestion, its question:
    /// the row's message only while that ask is the one still open
    last_use: At<(String, Option<String>)>,
}

fn since<T>(a: &At<T>, ws: u64) -> Option<&T> {
    a.as_ref().filter(|(o, _)| *o >= ws).map(|(_, v)| v)
}

impl Fold {
    /// Folds the row that starts at byte `off`.
    pub(super) fn step(&mut self, off: u64, raw: &str) {
        let Ok(v) = serde_json::from_str::<Value>(raw) else { return };
        // In case a future version inlines subagent rows.
        if v["isSidechain"].as_bool() == Some(true) || !v["agentId"].is_null() {
            return;
        }
        let ts = v["timestamp"].as_str().and_then(rfc3339);
        if let Some(t) = ts {
            self.newest = Some((off, t));
        }
        if let Some(c) = v["cwd"].as_str().filter(|c| !c.is_empty()) {
            self.cwd = Some((off, c.to_string()));
        }
        match v["type"].as_str() {
            Some("user") => {
                self.saw = Some(off);
                let c = &v["message"]["content"];
                match c.as_str() {
                    // a tool_result arrives as an array, a human turn as a string
                    Some(t) if t.starts_with(INTERRUPT) => self.turn_over = Some((off, true)),
                    // <task-notification, <command-name, <local-command-*: the
                    // harness wrote these, so not a new turn and not `asked`.
                    Some(t) if t.starts_with('<') => {}
                    Some(t) => {
                        self.user = Some((off, (first_line(t), ts)));
                        self.turn_over = Some((off, false));
                        // a typed prompt means the human moved on from an interrupted ask
                        self.asks.clear();
                    }
                    None => {
                        let (mut interrupted, mut result, mut typed) = (false, false, None);
                        for b in c.as_array().into_iter().flatten() {
                            if let Some(id) = b["tool_use_id"].as_str() {
                                self.asks.retain(|(_, a)| a != id);
                                result = true;
                            }
                            // safe beside the tool_results: they carry no `text`
                            if let Some(t) = b["text"].as_str() {
                                interrupted |= t.starts_with(INTERRUPT);
                                if typed.is_none() && !t.starts_with(INTERRUPT) && !t.starts_with('<') {
                                    typed = Some(t);
                                }
                            }
                        }
                        self.turn_over = Some((off, interrupted));
                        // A prompt with an image is an array too: its text block is the
                        // human's, unless the row is a tool_result or one the harness wrote.
                        if let Some(t) = typed
                            && !result
                            && !interrupted
                            && v["isMeta"].as_bool() != Some(true)
                            && !first_line(t).is_empty()
                        {
                            self.user = Some((off, (first_line(t), ts)));
                            self.asks.clear();
                        }
                    }
                }
            }
            Some("assistant") => {
                self.saw = Some(off);
                if let Some(t) = ts {
                    self.at = Some((off, t));
                }
                // end_turn rides the text row; attachment and custom-title rows
                // land after it and must not reopen the turn.
                self.turn_over = Some((off, v["message"]["stop_reason"].as_str() == Some("end_turn")));
                let Some(blocks) = v["message"]["content"].as_array() else {
                    return;
                };
                for b in blocks {
                    match b["type"].as_str() {
                        Some("tool_use") => {
                            let name = b["name"].as_str().unwrap_or("");
                            // The name alone when no field says more: never a bare `name:`.
                            let tool = match transcript::pick(name, &b["input"]).map(|(p, s)| (p, first_line(s))) {
                                Some((p, d)) if !d.is_empty() => format!("{name}: {p}{d}"),
                                _ => name.to_string(),
                            };
                            self.tool = Some((off, tool));
                            let id = b["id"].as_str().unwrap_or_default().to_string();
                            let ask = name == "AskUserQuestion";
                            if ask && !id.is_empty() {
                                self.asks.push((off, id.clone()));
                            }
                            self.last_use = Some((off, (id, ask.then(|| transcript::question(&b["input"])).flatten())));
                        }
                        Some("text") => {
                            let t = first_line(b["text"].as_str().unwrap_or(""));
                            if !t.is_empty() {
                                self.text = Some((off, t));
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    /// The facts of the rows that start at or after `ws`.
    pub(super) fn view(&self, ws: u64) -> Facts {
        let asks: Vec<&str> = self.asks.iter().filter(|(o, _)| *o >= ws).map(|(_, id)| id.as_str()).collect();
        let (user, user_at) = since(&self.user, ws).cloned().unwrap_or_default();
        let saw = self.saw.is_some_and(|o| o >= ws);
        let turn_over = since(&self.turn_over, ws).copied().unwrap_or(false);
        let ask = since(&self.last_use, ws).filter(|(id, _)| asks.contains(&id.as_str()));
        Facts {
            tool: since(&self.tool, ws).cloned().unwrap_or_default(),
            text: since(&self.text, ws).cloned().unwrap_or_default(),
            user,
            at: since(&self.at, ws).copied(),
            user_at,
            newest: since(&self.newest, ws).copied(),
            // An open question outranks a finished turn: the interrupt that ended the
            // turn left the picker's tool_use open, and the human still owes an answer.
            state: match () {
                _ if !asks.is_empty() => State::NeedsInput,
                // an unknown state is unknown, never idle
                _ if !saw => State::Unknown,
                _ if turn_over => State::Done,
                _ => State::Running,
            },
            cwd: since(&self.cwd, ws).cloned().unwrap_or_default(),
            ask: ask.and_then(|(_, q)| q.clone()).unwrap_or_default(),
        }
    }
}

//! The board's transcript fold (#492): what `parse` reads from the rows, each fact
//! kept with the byte offset of the row that last set it. A window that starts at
//! `ws` holds the rows that start at or after `ws`, so `view(ws)` answers for that
//! window from a fold over a longer stretch: a fact whose row lies before `ws` is
//! one the window never saw.

use super::{Facts, State, first_line, rfc3339, transcript};
use serde_json::Value;
#[cfg(test)]
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::{File, Metadata};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::time::SystemTime;

use super::transcript::INTERRUPT;

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
                // a tool_result closes its ask, whatever else the row is
                for id in c.as_array().into_iter().flatten().filter_map(|b| b["tool_use_id"].as_str()) {
                    self.asks.retain(|(_, a)| a != id);
                }
                // trimmed as transcript::typed_text trims it, so the two agree
                let interrupt = |t: &str| t.trim_start().starts_with(INTERRUPT);
                let interrupted = match c.as_str() {
                    Some(t) => interrupt(t),
                    None => c.as_array().into_iter().flatten().filter_map(|b| b["text"].as_str()).any(interrupt),
                };
                // #477: one rule with the info screen for a typed prompt, which means
                // the human moved on from an interrupted ask
                if let Some(p) = transcript::typed_prompt(&v) {
                    self.user = Some((off, (p, ts)));
                    self.turn_over = Some((off, false));
                    self.asks.clear();
                } else if interrupted || c.is_array() {
                    // the interrupt ends the turn; a tool_result keeps it going.
                    // Any other string is the harness's: not a turn and not `asked`.
                    self.turn_over = Some((off, interrupted));
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

/// One transcript's fold and where it stands in the file.
struct Track {
    /// (device, inode): another file at the path starts the fold over
    file: (u64, u64),
    len: u64,
    mtime: SystemTime,
    /// the fold holds every row that starts at or after `base`
    base: u64,
    /// just past the last newline folded: a partial row starts here
    pos: u64,
    /// the bytes from `pos` to the next newline end a row that started before `base`
    skip: bool,
    /// (length, hash) of the up to `PRINT` bytes before `pos`: another file
    /// written in place under the same inode, longer, differs there
    print: (u64, u64),
    fold: Fold,
}

/// The bytes before `pos` a read checks again, as a hash.
const PRINT: u64 = 1024;

fn hash(b: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    b.hash(&mut h);
    h.finish()
}

thread_local! {
    /// One per transcript path, like `TAILS`, and like it never evicted: a Track
    /// is small, its asks pruned to the 2 MB window.
    static TRACKS: RefCell<HashMap<String, Track>> = RefCell::new(HashMap::new());
    #[cfg(test)]
    static READ: Cell<u64> = const { Cell::new(0) };
}

/// Transcript bytes read on this thread so far: the delta read's audit.
#[cfg(test)]
pub(super) fn read_bytes() -> u64 {
    READ.with(Cell::get)
}

impl Track {
    fn new(file: (u64, u64), len: u64, mtime: SystemTime, base: u64) -> Self {
        let fold = Fold::default();
        let skip = base > 0;
        Track {
            file,
            len,
            mtime,
            base,
            pos: base,
            skip,
            print: (0, 0),
            fold,
        }
    }

    /// Reads `[pos, len)`, folds each whole row and returns the partial last one.
    /// None when the bytes before `pos` are not the ones the last read ended on, or
    /// the byte before it is no longer a newline: the file was rewritten.
    fn advance(&mut self, f: &mut File) -> Option<Vec<u8>> {
        let back = if self.print.0 > 0 { self.print.0 } else { self.pos.min(1) };
        let from = self.pos - back;
        f.seek(SeekFrom::Start(from)).ok()?;
        let mut buf = Vec::new();
        f.take(self.len - from).read_to_end(&mut buf).ok()?;
        #[cfg(test)]
        READ.with(|r| r.set(r.get() + buf.len() as u64));
        if (buf.len() as u64) < back {
            return None;
        }
        let (head, mut rest) = buf.split_at(back as usize);
        if self.print.0 > 0 && hash(head) != self.print.1 {
            return None;
        }
        if let Some(b) = head.last() {
            match (*b == b'\n', self.skip) {
                (true, _) => self.skip = false,
                (false, false) => return None,
                (false, true) => {}
            }
        }
        if self.skip {
            let Some(i) = rest.iter().position(|b| *b == b'\n') else {
                self.pos = self.len;
                self.mark(&buf);
                return Some(vec![]);
            };
            (self.pos, rest, self.skip) = (self.pos + i as u64 + 1, &rest[i + 1..], false);
        }
        while let Some(i) = rest.iter().position(|b| *b == b'\n') {
            let line = String::from_utf8_lossy(&rest[..i]);
            self.fold.step(self.pos, line.strip_suffix('\r').unwrap_or(&line));
            (self.pos, rest) = (self.pos + i as u64 + 1, &rest[i + 1..]);
        }
        self.mark(&buf[..(self.pos - from) as usize]);
        Some(rest.to_vec())
    }

    /// Remembers the up to `PRINT` bytes at the end of `upto`, which ends at `pos`.
    fn mark(&mut self, upto: &[u8]) {
        let tail = &upto[upto.len().saturating_sub(PRINT as usize)..];
        self.print = (tail.len() as u64, hash(tail));
    }

    /// The window from `ws`, the partial row counted as `tail` counts it.
    fn view(&self, ws: u64, partial: &[u8]) -> Facts {
        if partial.is_empty() {
            return self.fold.view(ws);
        }
        let mut f = self.fold.clone();
        f.step(self.pos, &String::from_utf8_lossy(partial));
        f.view(ws)
    }
}

/// What `parse(tail(f, len, first))` says, widened to `max` when that window holds
/// no assistant row, from the bytes appended since the last call (#492). The fold
/// starts over when another file stands at the path, the file shrank, it was
/// rewritten at the same length, or the window reaches back past the fold.
pub(super) fn windowed(path: &str, f: &mut File, md: &Metadata, first: u64, max: u64) -> Facts {
    let (file, len) = ((md.dev(), md.ino()), md.len());
    let mtime = md.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let ws = |cap: u64| len.saturating_sub(cap);
    let fresh = |cap: u64| Track::new(file, len, mtime, ws(cap));
    let kept = TRACKS.with(|c| c.borrow_mut().remove(path)).filter(|t| {
        let grew = len > t.len || (len == t.len && mtime == t.mtime);
        let near = len.checked_sub(t.pos).is_some_and(|d| d <= max);
        t.file == file && grew && near && t.base <= ws(first)
    });
    let mut t = kept.map(|t| Track { len, mtime, ..t }).unwrap_or_else(|| fresh(first));
    let mut partial = match t.advance(f) {
        Some(p) => p,
        None => {
            t = fresh(first);
            let Some(p) = t.advance(f) else { return Facts::default() };
            p
        }
    };
    let mut fa = t.view(ws(first), &partial);
    if fa.tool.is_empty() && fa.text.is_empty() && len > first && first < max {
        if t.base > ws(max) {
            t = fresh(max);
            let Some(p) = t.advance(f) else { return Facts::default() };
            partial = p;
        }
        fa = t.view(ws(max), &partial);
    }
    // No window starts before `len - max`: an ask older than that is never seen again.
    t.fold.asks.retain(|(o, _)| *o >= ws(max));
    TRACKS.with(|c| c.borrow_mut().insert(path.to_string(), t));
    fa
}

#[cfg(test)]
mod tests {
    use super::super::{TAIL_FIRST, TAIL_MAX, parse, tail};
    use super::*;
    use serde_json::json;
    use std::io::Write;
    use std::path::PathBuf;

    /// A scratch dir removed on drop.
    struct Tmp(PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn tmp(name: &str) -> (Tmp, String) {
        let d = std::env::temp_dir().join(format!("cockpit-fold-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("t.jsonl").display().to_string();
        std::fs::write(&p, "").unwrap();
        (Tmp(d), p)
    }

    /// Today's read: the window, widened to `TAIL_MAX` when it holds no assistant row.
    fn full(p: &str, first: u64) -> Facts {
        let mut f = File::open(p).unwrap();
        let len = f.metadata().unwrap().len();
        let mut fa = parse(tail(&mut f, len, first));
        if fa.tool.is_empty() && fa.text.is_empty() && len > first && first < TAIL_MAX {
            fa = parse(tail(&mut f, len, TAIL_MAX));
        }
        fa
    }
    fn delta(p: &str, first: u64) -> Facts {
        let mut f = File::open(p).unwrap();
        let md = f.metadata().unwrap();
        windowed(p, &mut f, &md, first, TAIL_MAX)
    }
    /// The delta read equals a full read, for the board's first window and a wider one.
    fn same(p: &str, what: &str) {
        for first in [TAIL_FIRST, TAIL_MAX / 2] {
            assert_eq!(delta(p, first), full(p, first), "{what}, first {first}");
        }
    }
    fn add(p: &str, s: &str) {
        std::fs::OpenOptions::new()
            .append(true)
            .open(p)
            .unwrap()
            .write_all(s.as_bytes())
            .unwrap();
    }

    fn ts(n: usize) -> String {
        format!("2026-11-06T10:{:02}:{:02}.000Z", n / 60 % 60, n % 60)
    }
    /// One row of type `kind` with timestamp `n`, its message `msg`.
    fn row(kind: &str, n: usize, msg: Value) -> String {
        json!({"type": kind, "timestamp": ts(n), "message": msg}).to_string() + "\n"
    }
    fn prompt(n: usize, cwd: &str) -> String {
        let msg = json!({"content": format!("prompt {n}")});
        json!({"type": "user", "timestamp": ts(n), "cwd": cwd, "message": msg}).to_string() + "\n"
    }
    /// A text block and a tool_use `u<n>`; an AskUserQuestion asks "q?".
    fn tool(n: usize, name: &str) -> String {
        let input = json!({"command": format!("cmd {n}"), "questions": [{"question": "q?"}]});
        let text = json!({"type": "text", "text": format!("said {n}")});
        let call = json!({"type": "tool_use", "id": format!("u{n}"), "name": name, "input": input});
        row("assistant", n, json!({"content": [text, call]}))
    }
    /// A tool_result of `kb` KB that closes `id`.
    fn result(n: usize, id: &str, kb: usize) -> String {
        let body = "x".repeat(kb * 1024);
        let block = json!({"type": "tool_result", "tool_use_id": id, "content": body});
        row("user", n, json!({"content": [block]}))
    }
    fn done(n: usize) -> String {
        let text = json!({"type": "text", "text": format!("done {n}")});
        row("assistant", n, json!({"content": [text], "stop_reason": "end_turn"}))
    }

    /// Row by row, from empty past `TAIL_MAX`: an open ask, then big results that
    /// push it, the tool and the prompt out of the 256 KB window (the 2 MB read
    /// finds them), then out of the 2 MB one too, then a new turn.
    #[test]
    fn delta_equals_a_full_read_as_rows_are_appended() {
        let (_d, p) = tmp("append");
        same(&p, "empty");
        let ask = tool(3, "AskUserQuestion");
        let mut rows = vec![prompt(0, "/w/a"), tool(1, "Bash"), result(2, "u1", 1), ask];
        rows.extend((4..40).map(|n| result(n, "other", 64)));
        rows.extend([prompt(40, "/w/b"), tool(41, "Read"), result(42, "u41", 1), done(43)]);
        for (n, r) in rows.iter().enumerate() {
            add(&p, r);
            same(&p, &format!("row {n}"));
        }
        assert_eq!(full(&p, TAIL_FIRST).state, State::Done, "the fixture ends the turn");
    }

    /// #477 review: an interrupt with blanks before it still ends the turn, as
    /// typed_text, which trims, says it is not a prompt.
    #[test]
    fn an_interrupt_after_blanks_still_ends_the_turn() {
        let (_d, p) = tmp("blanks");
        add(&p, &(prompt(0, "/w") + &tool(1, "Bash")));
        add(&p, &row("user", 2, json!({"content": "\n [Request interrupted by user]"})));
        assert_eq!(full(&p, TAIL_FIRST).state, State::Done);
    }

    /// #477: each kind of user row the shared typed-prompt rule weighs, after an
    /// open ask: the delta view equals a full read after every row.
    #[test]
    fn delta_equals_a_full_read_for_each_kind_of_user_row() {
        let (_d, p) = tmp("kinds");
        let image = json!({"type": "image", "source": {"type": "base64", "data": "AA=="}});
        let text = |t: &str| json!({"type": "text", "text": t});
        let meta = json!({"type": "user", "isMeta": true, "timestamp": ts(9), "message": {"content": "Caveat"}});
        let rows = [
            prompt(0, "/w"),
            tool(1, "AskUserQuestion"),
            row("user", 2, json!({"content": "[Request interrupted by user]"})),
            row("user", 3, json!({"content": "<task-notification>done</task-notification>"})),
            meta.to_string() + "\n",
            row("user", 4, json!({"content": [image, text("look")]})),
            tool(5, "AskUserQuestion"),
            row("user", 6, json!({"content": "<pasted_content>long</pasted_content>"})),
        ];
        for (n, r) in rows.iter().enumerate() {
            add(&p, r);
            same(&p, &format!("row {n}"));
        }
    }

    /// A row read before its newline lands counts as `tail` counts it, then the
    /// rest of it arrives.
    #[test]
    fn delta_equals_a_full_read_through_a_partial_row() {
        let (_d, p) = tmp("partial");
        add(&p, &(prompt(0, "/w") + &tool(1, "Bash")));
        same(&p, "whole rows");
        let r = done(2);
        let (a, b) = r.split_at(r.len() / 2);
        add(&p, a);
        same(&p, "half a row");
        add(&p, b.trim_end_matches('\n'));
        same(&p, "a whole row with no newline");
        add(&p, &("\n".to_string() + &tool(3, "Grep")));
        same(&p, "its newline and another row");
    }

    /// A shorter file at the same inode is a rewrite, not an append: here one cut
    /// inside the partial row, the `PRINT` bytes before the offset (a 2 KB result)
    /// as they were.
    #[test]
    fn delta_starts_over_when_the_file_shrinks() {
        let (_d, p) = tmp("shrink");
        let (r, pad) = (done(3), result(2, "u1", 2));
        add(&p, &(prompt(0, "/w/a") + &tool(1, "Bash") + &pad + &r[..r.len() / 2]));
        same(&p, "before");
        std::fs::OpenOptions::new().write(true).open(&p).unwrap().set_len(0).unwrap();
        add(&p, &(prompt(0, "/w/b") + &tool(1, "Read") + &pad + &r[..r.len() / 4]));
        same(&p, "rewritten shorter");
    }

    /// Cut below what was read, then rewritten.
    #[test]
    fn delta_starts_over_when_the_file_is_truncated() {
        let (_d, p) = tmp("truncate");
        add(&p, &(prompt(0, "/w/a") + &tool(1, "AskUserQuestion") + &result(2, "x", 4)));
        same(&p, "before");
        std::fs::OpenOptions::new().write(true).open(&p).unwrap().set_len(0).unwrap();
        add(&p, &(prompt(5, "/w/b") + &tool(6, "Bash")));
        same(&p, "rewritten shorter");
    }

    /// Another file renamed over the path, longer, its newlines and the `PRINT`
    /// bytes before the offset (a 2 KB result) where the old one had them: only the
    /// inode says it is another file.
    #[test]
    fn delta_starts_over_on_a_new_inode() {
        let (_d, p) = tmp("inode");
        let pad = result(2, "u1", 2);
        add(&p, &(prompt(0, "/w/a") + &tool(1, "Bash") + &pad));
        same(&p, "before");
        let next = format!("{p}.new");
        let body = prompt(0, "/w/b") + &tool(1, "Read") + &pad + &done(3);
        std::fs::write(&next, &body).unwrap();
        std::fs::rename(&next, &p).unwrap();
        same(&p, "replaced");
    }

    /// The same inode rewritten longer in place, a newline still where the last read
    /// ended: the bytes before that offset are another file's. Rows: the change in
    /// the last row read, and one row further back.
    #[test]
    fn delta_starts_over_when_rewritten_longer_in_place() {
        for (name, tail) in [("last row", String::new()), ("a row back", result(2, "u1", 0))] {
            let (_d, p) = tmp(&format!("longer-{}", name.replace(' ', "-")));
            add(&p, &(prompt(0, "/w/a") + &tool(1, "Bash") + &tail));
            same(&p, &format!("{name}, before"));
            std::fs::write(&p, prompt(0, "/w/b") + &tool(1, "Read") + &tail + &done(3)).unwrap();
            same(&p, &format!("{name}, rewritten longer"));
        }
    }

    /// The same length with a new mtime is a rewrite in place, the `PRINT` bytes
    /// before the offset (a 2 KB result) as they were.
    #[test]
    fn delta_starts_over_when_rewritten_at_the_same_length() {
        let (_d, p) = tmp("same-len");
        let pad = result(2, "u1", 2);
        add(&p, &(prompt(0, "/w/a") + &tool(1, "Bash") + &pad));
        same(&p, "before");
        let f = std::fs::OpenOptions::new().write(true).open(&p).unwrap();
        let body = prompt(0, "/w/b") + &tool(1, "Read") + &pad;
        (&f).write_all(body.as_bytes()).unwrap();
        f.set_modified(SystemTime::UNIX_EPOCH).unwrap();
        same(&p, "rewritten");
    }

    /// A 4 KB append to a 1 MB transcript reads the appended row, not the window.
    #[test]
    fn delta_reads_only_the_appended_bytes() {
        let (_d, p) = tmp("cost");
        add(&p, &(prompt(0, "/w") + &tool(1, "Bash")));
        (2..18).for_each(|n| add(&p, &result(n, "u1", 64)));
        delta(&p, TAIL_FIRST);
        let r = tool(18, "Read");
        add(&p, &r);
        let before = read_bytes();
        delta(&p, TAIL_FIRST);
        let read = read_bytes() - before;
        assert_eq!(read, r.len() as u64 + PRINT, "the row and the bytes checked before it");
    }

    /// One append past 256 KB, an open ask, then only tool_results: the first read
    /// finds no assistant row in its window and starts over at 2 MB.
    #[test]
    fn a_window_with_no_assistant_row_widens_to_tail_max() {
        let (_d, p) = tmp("widen");
        let results: String = (2..7).map(|n| result(n, "other", 64)).collect();
        add(&p, &(prompt(0, "/w/a") + &tool(1, "AskUserQuestion") + &results));
        assert_eq!(full(&p, TAIL_FIRST).state, State::NeedsInput, "the full read finds the ask");
        same(&p, "first read");
    }

    /// The fold starts at the 256 KB window; a 1 MB window reaches back past it.
    #[test]
    fn a_window_wider_than_the_fold_starts_over() {
        let (_d, p) = tmp("wider");
        let results: String = (2..12).map(|n| result(n, "u1", 64)).collect();
        add(&p, &(prompt(0, "/w/a") + &tool(1, "Read") + &results + &done(12)));
        assert_eq!(full(&p, TAIL_MAX / 2).cwd, "/w/a", "the wider window holds the prompt");
        same(&p, "256 KB, then 1 MB");
    }

    /// A 1 MB assistant row with no newline yet, starting before the 256 KB window:
    /// the full read drops it from that window and reads it, with the prompt before
    /// it, in the 2 MB one.
    #[test]
    fn a_long_partial_row_counts_from_where_it_starts() {
        let (_d, p) = tmp("long-partial");
        let mut r: Value = serde_json::from_str(&done(1)).unwrap();
        r["pad"] = "x".repeat(1 << 20).into();
        add(&p, &(prompt(0, "/w/a") + &r.to_string()));
        assert_eq!(full(&p, TAIL_FIRST).cwd, "/w/a", "the 2 MB read holds the prompt");
        same(&p, "no newline");
    }

    /// More than 2 MB appended at once: the read is the window, not the append.
    #[test]
    fn an_append_past_tail_max_reads_the_window() {
        let (_d, p) = tmp("cap");
        add(&p, &(prompt(0, "/w") + &tool(1, "Bash")));
        delta(&p, TAIL_FIRST);
        let results: String = (2..36).map(|n| result(n, "u1", 64)).collect();
        add(&p, &(results + &tool(36, "Read")));
        let before = read_bytes();
        delta(&p, TAIL_FIRST);
        assert_eq!(read_bytes() - before, TAIL_FIRST + 1, "the window and the byte before it");
    }
}

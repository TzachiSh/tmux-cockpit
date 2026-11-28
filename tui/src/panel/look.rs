//! Explain's what-changed list (#1000, step 2 of #935). The collect runs on a
//! thread of its own once the user heads for Explain, and the panel draws the
//! list atop that Explain's first answer when it lands. No call waits for it,
//! and the request does not carry it: #935 task 4 adds it with its prompt
//! text. It is marked seen once a frame showed it.
//!
//! The panel holds one [`Collector`] for its life, so the 60 s floor and the
//! searches a minute are this process's: a new panel starts with none spent.
//! #1000 allows no file but the last-seen one (#1003's F5).
use super::changes::{Changes, Collector, Note, mark_seen, utc};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long past the collector's own two budgets its thread may still answer.
pub const SLACK: Duration = Duration::from_secs(1);

/// The panel's collector and the list of the Explain under way.
pub struct Looks {
    collector: Arc<Mutex<Collector>>,
    /// The collector's git and gh budgets and [`SLACK`]: a collect's longest time.
    wait: Duration,
    store: Option<PathBuf>,
    cur: Option<Look>,
}

struct Look {
    rx: Option<Receiver<(Changes, u64)>>,
    until: Instant,
    /// The list, and the collector's time when the collect started.
    got: Option<(Changes, u64)>,
    /// An answer drew it: the next Explain collects again.
    done: bool,
}

impl Looks {
    pub fn new(c: Collector) -> Looks {
        Looks {
            wait: c.git_budget + c.gh.budget + SLACK,
            store: c.store.clone(),
            collector: Arc::new(Mutex::new(c)),
            cur: None,
        }
    }

    /// Starts a collect for the repo `dir` is in, unless one is under way or
    /// its list is not yet seen: a cancelled or failed Explain keeps its list.
    /// A thread that ended with no list is started again.
    pub fn ahead(&mut self, dir: &str) {
        self.poll();
        let gone = |l: &Look| l.rx.is_none() && l.got.is_none();
        if self.cur.as_ref().is_some_and(|l| !l.done && !gone(l)) {
            return;
        }
        let (tx, rx) = channel();
        let (c, dir) = (self.collector.clone(), dir.to_string());
        std::thread::spawn(move || {
            let mut c = c.lock().unwrap_or_else(|e| e.into_inner());
            let at = c.now();
            let _ = tx.send((c.collect(&dir), at));
        });
        self.cur = Some(Look {
            rx: Some(rx),
            until: Instant::now() + self.wait,
            got: None,
            done: false,
        });
    }

    /// Takes the list if the thread has sent it. Never waits.
    pub fn poll(&mut self) {
        let Some(l) = self.cur.as_mut() else { return };
        let Some(rx) = &l.rx else { return };
        match rx.try_recv() {
            Ok(got) => (l.got, l.rx) = (Some(got), None),
            Err(TryRecvError::Disconnected) => l.rx = None,
            Err(TryRecvError::Empty) => {}
        }
    }

    /// A collect is under way, inside the collector's budgets: the loop ticks
    /// so the list is drawn when it lands.
    pub fn waiting(&self) -> bool {
        self.cur.as_ref().is_some_and(|l| l.rx.is_some() && Instant::now() < l.until)
    }

    /// A collect started, or its list in, that no answer has drawn yet. A list
    /// that can never be marked seen is none ([`unmarkable`]).
    pub fn unseen(&self) -> bool {
        let none = |l: &Look| l.got.as_ref().is_some_and(|(c, _)| unmarkable(c));
        self.cur.as_ref().is_some_and(|l| !l.done && !none(l))
    }

    /// The heading and the lines the Answer draws, once the list is in; none for
    /// a list that can never be marked seen ([`unmarkable`]).
    pub fn shown(&self) -> Option<(String, Vec<String>)> {
        let (c, _) = self.cur.as_ref()?.got.as_ref()?;
        if unmarkable(c) {
            return None;
        }
        let head = match (&c.seen, c.items.is_empty()) {
            (Some(s), false) => format!("what changed since you last looked, {}", utc(s.at)),
            (Some(s), true) => format!("nothing new listed since you last looked, {}", utc(s.at)),
            (None, false) => "first look here: the newest commits".to_string(),
            (None, true) => "first look here: nothing listed".to_string(),
        };
        Some((head, c.lines()))
    }

    /// An answer drew the list: the record [`mark_at`] allows is written,
    /// once. `None` when nothing was to be written, or no folder holds it.
    pub fn drawn(&mut self) -> Option<std::io::Result<()>> {
        let l = self.cur.as_mut().filter(|l| l.got.is_some() && !l.done)?;
        l.done = true;
        let (c, at) = l.got.as_ref()?;
        let at = mark_at(c, *at)?;
        let (key, head) = (c.key.as_deref()?, c.head.as_deref()?);
        Some(mark_seen(self.store.as_deref()?, key, head, at))
    }
}

/// A list with no repo key (outside any repo) or no head (no commit yet): no
/// mark could ever be written, so it would open every Explain as a first look
/// (#1269). The panel shows none. A head missing because git took too long is
/// not that: the list keeps its GitHub items and says git was slow.
fn unmarkable(c: &Changes) -> bool {
    c.key.is_none() || (c.head.is_none() && !c.notes.contains(&Note::GitTimedOut))
}

/// The time a mark of `c` carries, `at` being when its collect started; `None`
/// writes nothing. Commits left out (git took too long) keep the old record.
/// A passing GitHub failure moves the head but keeps the old time, so the next
/// look searches from where the user last saw GitHub.
pub fn mark_at(c: &Changes, at: u64) -> Option<u64> {
    if c.key.is_none() || c.head.is_none() || c.notes.contains(&Note::GitTimedOut) {
        return None;
    }
    use Note::{Floor, GhFailed, GhSilent, GhTimedOut, GhUnreadable, SearchLimit};
    let limit = |n: &Note| matches!(n, Floor { .. } | SearchLimit { .. });
    let gh = [GhTimedOut, GhFailed, GhSilent, GhUnreadable];
    let passing = c.notes.iter().any(|n| gh.contains(n) || limit(n));
    match (&c.seen, passing) {
        (Some(s), true) => Some(s.at),
        _ => Some(at),
    }
}

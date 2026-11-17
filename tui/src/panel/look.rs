//! Explain's what-changed list (#1000, step 2 of #935). The collect runs on a
//! thread of its own once the user heads for Explain. Each call of that Explain
//! sends the list, and the first answer draws it at its top. It is marked seen
//! once a frame showed it.
//!
//! The panel holds one [`Collector`] for its life, so the 60 s floor and the
//! searches a minute are this process's: a new panel starts with none spent.
//! #1000 allows no file but the last-seen one (#1003's F5).
use super::changes::{Changes, Collector, Note, mark_seen, utc};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A call waits this long past the collector's own two budgets.
pub const SLACK: Duration = Duration::from_secs(1);

/// The panel's collector and the list of the Explain under way.
pub struct Looks {
    collector: Arc<Mutex<Collector>>,
    /// The collector's git and gh budgets and [`SLACK`]: a call's longest wait.
    wait: Duration,
    store: PathBuf,
    cur: Option<Look>,
}

struct Look {
    rx: Option<Receiver<(Changes, u64)>>,
    until: Instant,
    /// The list, and the collector's time when the collect started.
    got: Option<(Changes, u64)>,
    /// It went with a call, so that Explain's answer draws it.
    sent: bool,
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
    pub fn ahead(&mut self, dir: &str) {
        self.poll();
        if self.cur.as_ref().is_some_and(|l| !l.done) {
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
            sent: false,
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

    /// A collect is under way and inside its wait: a call holds for it.
    pub fn waiting(&self) -> bool {
        self.cur.as_ref().is_some_and(|l| l.rx.is_some() && Instant::now() < l.until)
    }

    /// A collect was started and its wait ran out with no list.
    pub fn late(&self) -> bool {
        self.cur.as_ref().is_some_and(|l| l.got.is_none() && !self.waiting())
    }

    /// The `<changes>` body a call sends, once the list is in.
    pub fn body(&self) -> Option<String> {
        let (c, _) = self.cur.as_ref()?.got.as_ref()?;
        let since = match &c.seen {
            Some(s) => format!("last looked {}", utc(s.at)),
            None => "last looked never; the newest commits".to_string(),
        };
        let none = c.items.is_empty().then(|| "nothing new listed".to_string());
        let lines: Vec<String> = [since].into_iter().chain(none).chain(c.lines()).collect();
        Some(lines.join("\n"))
    }

    /// The list went with a call.
    pub fn sent(&mut self) {
        if let Some(l) = self.cur.as_mut().filter(|l| l.got.is_some()) {
            l.sent = true;
        }
    }

    /// A list that went with a call and no answer has drawn yet.
    pub fn unseen(&self) -> bool {
        self.cur.as_ref().is_some_and(|l| l.sent && !l.done)
    }

    /// The heading and the lines the Answer draws: only a list that went with a call.
    pub fn shown(&self) -> Option<(String, Vec<String>)> {
        let l = self.cur.as_ref().filter(|l| l.sent)?;
        let (c, _) = l.got.as_ref()?;
        let head = match (&c.seen, c.items.is_empty()) {
            (Some(s), false) => format!("what changed since you last looked, {}", utc(s.at)),
            (Some(s), true) => format!("nothing new listed since you last looked, {}", utc(s.at)),
            (None, false) => "first look here: the newest commits".to_string(),
            (None, true) => "first look here: nothing listed".to_string(),
        };
        Some((head, c.lines()))
    }

    /// An answer drew the list: the record [`mark_at`] allows is written,
    /// once. `None` when nothing was to be written.
    pub fn drawn(&mut self) -> Option<std::io::Result<()>> {
        let l = self.cur.as_mut().filter(|l| l.sent && !l.done)?;
        l.done = true;
        let (c, at) = l.got.as_ref()?;
        let at = mark_at(c, *at)?;
        let (key, head) = (c.key.as_deref()?, c.head.as_deref()?);
        Some(mark_seen(&self.store, key, head, at))
    }
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

//! The session list behind `cockpit-tui sessions`: one `list-panes -a` read,
//! grouped into sessions and their windows. Pure parsing apart from `fetch`.
use super::{State, tmux};
use std::time::{SystemTime, UNIX_EPOCH};

const US: char = '\u{1f}';

/// One pane per line. The name fields come last and are split with a limit, so
/// a separator inside a name cannot shift the fields before it. An agent is a
/// pane whose command is in `@cockpit_agents_glob`, the test agent-state uses:
/// a stale `@panestate` on a shell is not believed. `@panestate_since` (#225) is
/// the epoch second a pane entered needs-input, written by agent-state alone.
fn format() -> String {
    let agent = "#{?#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}},1,0}";
    let msg = "#{?#{@panestate_msg},#{s/\n/ /:#{s/\t/ /:#{s/\u{1f}/ /:#{@panestate_msg}}}},}";
    [
        "#{session_id}",
        "#{session_attached}",
        "#{?session_last_attached,#{session_last_attached},#{session_created}}",
        "#{window_index}",
        "#{window_active}",
        "#{pane_id}",
        agent,
        "#{@panestate}",
        msg,
        // a separator in a hand-set value must not shift the name fields after it
        "#{s/\u{1f}//:#{@panestate_since}}",
        "#{window_name}",
        "#{session_name}",
    ]
    .join(&US.to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Win {
    pub idx: String,
    pub name: String,
    pub active: bool,
    pub agents: usize,
    /// Agent panes waiting on you.
    pub waiting: usize,
    /// The worst agent state in the window; `Unknown` with `agents > 0` is idle.
    pub state: State,
    /// The worst pane's message, when it needs you or failed.
    pub ask: String,
    /// The oldest `@panestate_since` of its waiting agent panes (#225).
    pub since: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sess {
    pub id: String,
    pub name: String,
    pub attached: usize,
    /// Epoch seconds of the last attach, else of creation.
    pub last: u64,
    pub wins: Vec<Win>,
}

impl Sess {
    pub fn agents(&self) -> usize {
        self.wins.iter().map(|w| w.agents).sum()
    }

    /// The worst agent state over every pane, `None` when there is no agent.
    pub fn state(&self) -> Option<State> {
        self.wins.iter().filter(|w| w.agents > 0).map(|w| w.state).min()
    }

    /// Waiting on you, or failed: the band that sorts first.
    pub fn urgent(&self) -> bool {
        matches!(self.state(), Some(State::NeedsInput | State::Error))
    }

    pub fn waiting(&self) -> usize {
        self.wins.iter().map(|w| w.waiting).sum()
    }

    /// When its longest-waiting pane entered needs-input; `None` when nothing waits
    /// or no waiting pane carries a stamp (one from before #225).
    pub fn since(&self) -> Option<u64> {
        if self.state() != Some(State::NeedsInput) {
            return None;
        }
        self.wins.iter().filter_map(|w| w.since).min()
    }

    /// Seconds its longest-waiting pane has waited. Wall clock: a stamp from the
    /// future (the clock went back) reads 0.
    pub fn wait(&self, now: u64) -> Option<u64> {
        self.since().map(|t| now.saturating_sub(t))
    }

    /// Seconds since last used.
    pub fn age(&self, now: u64) -> u64 {
        now.saturating_sub(self.last)
    }

    pub fn count(&self, s: State) -> usize {
        self.wins.iter().filter(|w| w.agents > 0 && w.state == s).count()
    }
}

/// `^[$][0-9]+$`: the only shape a session target takes on the action line.
pub fn is_sid(s: &str) -> bool {
    s.strip_prefix('$')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The sessions in the order tmux listed them. A line that does not split into
/// its fields is skipped, not guessed at.
pub fn parse(out: &str) -> Vec<Sess> {
    let mut v: Vec<Sess> = vec![];
    for l in out.lines() {
        let f: Vec<&str> = l.splitn(12, US).collect();
        let [id, att, last, widx, wact, _pane, agent, st, msg, since, wname, name] = f[..] else {
            continue;
        };
        let (Ok(att), Ok(last)) = (att.parse(), last.parse()) else {
            continue;
        };
        if !is_sid(id) {
            continue;
        }
        let at = match v.iter().position(|s| s.id == id) {
            Some(i) => i,
            None => {
                v.push(Sess {
                    id: id.into(),
                    name: name.into(),
                    attached: att,
                    last,
                    wins: vec![],
                });
                v.len() - 1
            }
        };
        let s = &mut v[at];
        let wi = match s.wins.iter().position(|w| w.idx == widx) {
            Some(i) => i,
            None => {
                s.wins.push(Win {
                    idx: widx.into(),
                    name: wname.into(),
                    active: wact == "1",
                    agents: 0,
                    waiting: 0,
                    state: State::Unknown,
                    ask: String::new(),
                    since: None,
                });
                s.wins.len() - 1
            }
        };
        if agent != "1" {
            continue;
        }
        let w = &mut s.wins[wi];
        let st: State = st.into();
        // the first pane of the worst state speaks for the window
        if w.agents == 0 || st < w.state {
            w.state = st;
            w.ask = if matches!(st, State::NeedsInput | State::Error) {
                msg.into()
            } else {
                String::new()
            };
        }
        w.agents += 1;
        w.waiting += usize::from(st == State::NeedsInput);
        if st == State::NeedsInput
            && let Ok(t) = since.parse::<u64>()
        {
            w.since = Some(w.since.map_or(t, |o| o.min(t)));
        }
    }
    v
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// `None` when tmux did not answer: no server is not "no sessions".
pub fn fetch() -> Option<Vec<Sess>> {
    tmux(&["list-panes", "-a", "-F", &format()]).map(|o| parse(&o))
}

/// Needs-you first (waiting, the longest wait first and the unstamped after
/// them, then failed), then the most recently used; the session this popup was
/// opened from goes last.
pub fn order(v: &mut [Sess], current: Option<&str>, now: u64) {
    let band = |s: &Sess| match s.state() {
        Some(State::NeedsInput) => 0,
        Some(State::Error) => 1,
        _ => 2,
    };
    // the earliest stamp is the longest wait; None sorts after every Some
    let wait = |s: &Sess| s.since().map_or((1, 0), |t| (0, t));
    v.sort_by_key(|s| (current == Some(s.id.as_str()), band(s), wait(s), s.age(now)));
}

/// The frozen order: the ids in the order of the first read. A refresh keeps
/// every session that is still there where it was, so a digit and the cursor keep
/// their session, drops the ones that went, and puts new ones at the end.
/// `only` (the `--needs` view) takes no new ones.
pub fn freeze(ids: &[String], fresh: Vec<Sess>, only: bool) -> Vec<Sess> {
    let mut late = vec![];
    let mut at: Vec<Option<Sess>> = vec![None; ids.len()];
    for s in fresh {
        match ids.iter().position(|i| *i == s.id) {
            Some(k) => at[k] = Some(s),
            None if !only => late.push(s),
            None => {}
        }
    }
    at.into_iter().flatten().chain(late).collect()
}

/// The `--needs` set: sessions waiting or failed; when none is, every session
/// (`false` says so) rather than an empty popup.
pub fn needs(v: Vec<Sess>) -> (Vec<Sess>, bool) {
    if v.iter().any(Sess::urgent) {
        (v.into_iter().filter(Sess::urgent).collect(), true)
    } else {
        (v, false)
    }
}

/// A new session's name: tmux forbids `.` and `:`, reads a leading `-` as an
/// option, and an escape typed at the prompt must not become a byte of it. The
/// same rules as `clean()` in scripts/tm, which checks again before it acts.
pub fn clean_name(s: &str) -> String {
    let n: String = s
        .chars()
        .filter(|c| !c.is_control())
        .map(|c| if matches!(c, '.' | ':' | ' ') { '_' } else { c })
        .collect();
    n.trim_start_matches('-').to_string()
}

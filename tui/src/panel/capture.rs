//! The target pane as the panel sees it, and what a Claude Code session on it
//! is doing, read off its own UI text.
use crate::data::tmux;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Capture {
    pub name: String,
    pub cmd: String,
    pub path: String,
    /// Raw, not redacted: the envelope redacts it on the way out.
    pub screen: String,
}

pub fn is_claude(name: &str, cmd: &str) -> bool {
    cmd == "claude" || name.eq_ignore_ascii_case("claude")
}

/// `None` when tmux cannot answer for the pane.
pub fn capture(pane: &str) -> Option<Capture> {
    let info = tmux(&[
        "display",
        "-p",
        "-t",
        pane,
        "#{window_name}\t#{pane_current_command}\t#{pane_current_path}",
    ])?;
    // Only the newline goes: a trim would eat the tab of an empty window name.
    let mut f = info.trim_end_matches(['\n', '\r']).splitn(3, '\t').map(String::from);
    let (name, cmd, path) = (
        f.next().unwrap_or_default(),
        f.next().unwrap_or_default(),
        f.next().unwrap_or_default(),
    );
    // A session's last answer and plan sit further back than a shell's last command.
    let depth = if is_claude(&name, &cmd) { "-200" } else { "-80" };
    let out = tmux(&["capture-pane", "-p", "-J", "-t", pane, "-S", depth])?;
    let mut lines: Vec<&str> = out.trim_end_matches('\n').split('\n').collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    Some(Capture {
        name,
        cmd,
        path,
        screen: lines.join("\n"),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Session {
    WaitingChoice,
    Working,
    Error,
    Asking,
    Ready,
}

impl Session {
    /// The `state` attribute the explain prompt documents.
    pub fn as_str(self) -> &'static str {
        match self {
            Session::WaitingChoice => "waiting_choice",
            Session::Working => "working",
            Session::Error => "error",
            Session::Asking => "asking",
            Session::Ready => "ready",
        }
    }

    pub fn text(self) -> &'static str {
        match self {
            Session::WaitingChoice => "is waiting for the user to pick an option in a permission or choice menu",
            Session::Working => "is working on a task right now",
            Session::Error => "stopped with an error",
            Session::Asking => "finished its turn with a question to the user",
            Session::Ready => "is idle, waiting for the next prompt",
        }
    }
}

/// From the screen, not `@panestate`: the hooks cannot tell a menu from a
/// question, and the prompt's five states need that split.
pub fn session_state(screen: &str, name: &str, cmd: &str) -> Option<Session> {
    if !is_claude(name, cmd) {
        return None;
    }
    let all: Vec<&str> = screen.split('\n').filter(|l| !l.trim().is_empty()).collect();
    let tail = &all[all.len().saturating_sub(25)..];
    if waiting_choice(tail) {
        return Some(Session::WaitingChoice);
    }
    if working(tail) {
        return Some(Session::Working);
    }
    if errored(tail) {
        return Some(Session::Error);
    }
    let said = tail.iter().map(|l| l.trim()).rfind(|l| l.starts_with('●'));
    Some(if said.is_some_and(|l| l.ends_with('?')) {
        Session::Asking
    } else {
        Session::Ready
    })
}

/// `N.` then whitespace or the end of the line.
fn numbered(l: &str, n: char) -> Option<&str> {
    let r = l.trim_start().strip_prefix(n)?.strip_prefix('.')?;
    (r.is_empty() || r.starts_with(char::is_whitespace)).then_some(r)
}

fn waiting_choice(tail: &[&str]) -> bool {
    const ASKS: [&str; 5] = ["proceed", "make this edit", "create", "run", "allow"];
    let asked = tail.iter().any(|l| {
        l.match_indices("Do you want to ")
            .any(|(i, m)| ASKS.iter().any(|a| l[i + m.len()..].starts_with(a)))
    });
    let cursor = tail.iter().any(|l| {
        let t = l.trim_start();
        t.strip_prefix(['❯', '>']).is_some_and(|r| numbered(r, '1').is_some())
    });
    let yes_no = tail.windows(2).any(|w| {
        let yes = numbered(w[0], '1').is_some_and(|r| {
            let t = r.trim_start();
            t.len() < r.len() && t.starts_with("Yes") && super::boundary_at(t, 3)
        });
        yes && numbered(w[1], '2').is_some()
    });
    asked || cursor || yes_no
}

fn working(tail: &[&str]) -> bool {
    tail.iter().any(|l| {
        if l.contains("esc to interrupt") {
            return true;
        }
        // `(12s ·`, the elapsed counter of a running turn.
        let timer = l.match_indices('(').any(|(i, _)| {
            let r = &l[i + 1..];
            let d = r.bytes().take_while(u8::is_ascii_digit).count();
            d > 0 && r[d..].strip_prefix('s').is_some_and(|r| r.trim_start().starts_with('·'))
        });
        // `✻ Thinking…`, the spinner line.
        let t = l.trim_start();
        let spinner = t.strip_prefix(['✻', '✶', '✳', '✢', '✽', '·', '*']).is_some_and(|r| {
            let w = r.trim_start();
            let word: usize = w.chars().take_while(|c| super::is_word(*c)).map(char::len_utf8).sum();
            w.len() < r.len() && word > 0 && w[word..].starts_with('…')
        });
        timer || spinner
    })
}

fn errored(tail: &[&str]) -> bool {
    tail.iter().any(|l| {
        let low = l.to_lowercase();
        let bullet = l
            .trim_start()
            .strip_prefix('⎿')
            .is_some_and(|r| r.trim_start().to_lowercase().starts_with("error"));
        low.contains("api error") || low.contains("request failed") || low.contains("rate limit") || bullet
    })
}

/// The window kind the write examples are picked by.
pub fn window_kind(name: &str, cmd: &str) -> &'static str {
    if is_claude(name, cmd) {
        "claude"
    } else if ["bash", "zsh", "fish", "sh"].contains(&cmd) {
        "shell"
    } else if ["ssh", "proot", "coder", "mosh", "mosh-client", "autossh"].contains(&cmd) {
        "ssh"
    } else {
        "other"
    }
}

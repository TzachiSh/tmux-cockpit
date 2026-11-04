//! The CLI. It never mutates: it prints `<verb> <pane-id>` and a shell wrapper
//! runs it.
use cockpit_tui::panel::{action, app, capture::capture, view};
use cockpit_tui::{board, data, fleet, theme};

/// Exit 1 with no stdout is "no tmux", so a wrapper can tell it from `[]`.
fn panes() -> Vec<data::Pane> {
    match data::list_panes() {
        Some(ps) => ps,
        None => {
            eprintln!("cockpit-tui: tmux is not reachable");
            std::process::exit(1);
        }
    }
}

const USAGE: &str = "cockpit-tui board [--select %N] [--note <file>] | pick | branch [-C <dir>] | info [--text] %N | sessions [--needs] [--current $N] | panel %N [--resume <state> --last ok|failed:<why>|hidden] | view [--line N] [--] <file> | --rows | --json | --version | -h";

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn is_pane(p: &str) -> bool {
    p.strip_prefix('%')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Never lists a wake channel in @cockpit_board_wake: those are the board's.
fn panel(args: &[String]) {
    let resume = match args.get(1..).unwrap_or_default() {
        [] => None,
        [r, f, l, last] if r == "--resume" && l == "--last" => Some((f.as_str(), last.as_str())),
        _ => {
            eprintln!("cockpit-tui: panel takes a pane id, then only --resume <state> --last <how>\n{USAGE}");
            std::process::exit(2);
        }
    };
    let Some(pane) = args.first().map(String::as_str).filter(|p| is_pane(p)) else {
        eprintln!("cockpit-tui: panel needs a pane id like %3\n{USAGE}");
        std::process::exit(2);
    };
    // checked before the capture: a bad word is the user's to fix, not a pane problem
    let mode = std::env::var("COCKPIT_PANEL_MODE").ok();
    let tablet = match cockpit_tui::panel::tablet_mode(mode.as_deref(), cockpit_tui::panel::is_termux()) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("cockpit-tui: {e}");
            std::process::exit(2);
        }
    };
    let Some(cap) = capture(pane) else {
        eprintln!("cockpit-tui: cannot capture pane {pane}");
        std::process::exit(1);
    };
    let mut a = app::App::new(pane, cap);
    a.tablet = tablet;
    a.termux = cockpit_tui::panel::is_termux();
    // other panes' hidden panels the wrapper dropped: their panes are gone
    if let Ok(gone) = std::env::var("COCKPIT_PANEL_DROPPED") {
        let gone: Vec<&str> = gone.split(' ').filter(|p| is_pane(p)).take(8).collect();
        if !gone.is_empty() {
            a.say(
                app::Level::Info,
                format!("dropped the panel hidden on {}: the pane is gone", gone.join(" ")),
            );
        }
    }
    if let Some((f, "hidden")) = resume {
        a.reopened = true;
        match action::read_hidden(std::path::Path::new(f), pane, unix_now()) {
            Ok(h) => a.unhide(h),
            Err(e) => {
                eprintln!("cockpit-tui: cannot bring the hidden panel back from {f}: {e}");
                a.say(app::Level::Warn, format!("the hidden panel was dropped: {e}"));
            }
        }
    } else if let Some((f, last)) = resume {
        a.reopened = true;
        match action::read_state(std::path::Path::new(f), pane) {
            Ok((cmd, r)) => a.resumed(cmd, r, last),
            Err(e) => {
                eprintln!("cockpit-tui: cannot resume from {f}: {e}");
                a.say(app::Level::Error, format!("could not reopen the steps: {e}"));
            }
        }
    }
    view::run(&mut a);
    if a.hide {
        let Some(f) = std::env::var_os("COCKPIT_ACTION").filter(|f| !f.is_empty()) else {
            eprintln!("cockpit-tui: hiding needs COCKPIT_ACTION to carry the state; it is lost");
            std::process::exit(1);
        };
        let kept = action::hide_body(&a, unix_now()).ok_or_else(|| "too much to keep".to_string());
        if let Err(e) = kept.and_then(|b| action::write_hidden(std::path::Path::new(&f), &b).map_err(|e| e.to_string())) {
            eprintln!("cockpit-tui: cannot keep the hidden panel: {e}");
            std::process::exit(1);
        }
        return;
    }
    let Some(act) = a.action else { return };
    // The text needs a file beside the action line; stdout has nowhere to put it.
    let Some(f) = std::env::var_os("COCKPIT_ACTION").filter(|f| !f.is_empty()) else {
        eprintln!("cockpit-tui: {} needs COCKPIT_ACTION to carry its text; nothing was sent", act.verb);
        std::process::exit(1);
    };
    let f = std::path::Path::new(&f);
    if let Some(r) = &a.resume
        && let Err(e) = action::write_state(f, pane, &a.cap.cmd, r)
    {
        // the action still goes: only the way back to the steps is lost
        eprintln!("cockpit-tui: cannot write the steps to resume: {e}");
    }
    if let Err(e) = action::write_action(f, pane, &a.cap.cmd, &act) {
        eprintln!("cockpit-tui: cannot write the action: {e}");
        std::process::exit(1);
    }
}

fn main() {
    // #295: SIGUSR1's default action is terminate. Nothing sends it since #224,
    // but an agent-state from before then still signals a @cockpit_tui_pid an
    // older board left standing, and that pid can be any cockpit-tui now.
    // SAFETY: signal() with SIG_IGN installs no code and touches no memory.
    unsafe { libc::signal(libc::SIGUSR1, libc::SIG_IGN) };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let action: Option<board::Exit> = match args.first().map(String::as_str) {
        Some("board") => match board::opts(&args[1..]) {
            Ok(o) => board::run(&o),
            Err(e) => {
                eprintln!("cockpit-tui: {e}\n{USAGE}");
                std::process::exit(2);
            }
        },
        Some("info") => std::process::exit(cockpit_tui::info::main(&args[1..])),
        Some("pick") => std::process::exit(cockpit_tui::pick::main(&args[1..])),
        // branch popup (#246)
        Some("branch") => std::process::exit(cockpit_tui::branch::main(&args[1..])),
        // sessions view (#245): writes its own action files, like panel
        Some("sessions") => std::process::exit(cockpit_tui::sessions::main(&args[1..])),
        // the markdown viewer (#283): prints `edit <line>` on stdout, as pick prints its key
        Some("view") => std::process::exit(cockpit_tui::mdview::main(&args[1..])),
        Some("panel") => {
            panel(&args[1..]);
            None
        }
        Some("--rows") => {
            println!("{}", fleet::rows_text(&panes(), &theme::Theme::fixed()));
            None
        }
        Some("--json") => {
            println!("{}", fleet::rows_json(&panes()));
            None
        }
        Some("--version") => {
            println!("cockpit-tui {}", env!("CARGO_PKG_VERSION"));
            None
        }
        Some("git-flags") => std::process::exit(data::gitsafe::git_flags(&args[1..])),
        Some("diffstat") => std::process::exit(data::gitsafe::diffstat(&args[1..])),
        Some("-h") | Some("--help") => {
            println!("{USAGE}");
            None
        }
        other => {
            eprintln!("cockpit-tui: unknown screen {}\n{USAGE}", other.unwrap_or(""));
            std::process::exit(2);
        }
    };
    // ratatui draws to stdout, where $(...) would swallow the UI: use a file.
    // `.expect` goes first, so the wrapper never reads a line whose value is not there yet.
    if let Some(a) = action {
        match std::env::var("COCKPIT_ACTION") {
            Ok(f) if !f.is_empty() => {
                if let Some(e) = &a.expect
                    && let Err(err) = action::write_expect(std::path::Path::new(&f), e)
                {
                    eprintln!("cockpit-tui: cannot write the expected value: {err}; nothing was sent");
                    std::process::exit(1);
                }
                let _ = std::fs::write(f, format!("{}\n", a.line));
            }
            _ if a.expect.is_some() => {
                eprintln!(
                    "cockpit-tui: {} needs COCKPIT_ACTION to carry its expected value; nothing was sent",
                    a.line
                );
                std::process::exit(1);
            }
            _ => println!("{}", a.line),
        }
    }
}

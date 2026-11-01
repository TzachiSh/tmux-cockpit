//! The CLI. It never mutates: it prints `<verb> <pane-id>` and a shell wrapper
//! runs it.
use cockpit_tui::panel::{app, capture::capture, view};
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

const USAGE: &str = "cockpit-tui board | panel %N [--resume <state> --last ok|failed:<why>] | --rows | --json | --version | -h";

/// Never publishes @cockpit_tui_pid: that pid is the board's, for its wakeups.
fn panel(args: &[String]) {
    let resume = match args.get(1..).unwrap_or_default() {
        [] => None,
        [r, f, l, last] if r == "--resume" && l == "--last" => Some((f.as_str(), last.as_str())),
        _ => {
            eprintln!("cockpit-tui: panel takes a pane id, then only --resume <state> --last <how>\n{USAGE}");
            std::process::exit(2);
        }
    };
    let Some(pane) = args.first().map(String::as_str).filter(|p| {
        p.strip_prefix('%')
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    }) else {
        eprintln!("cockpit-tui: panel needs a pane id like %3\n{USAGE}");
        std::process::exit(2);
    };
    let Some(cap) = capture(pane) else {
        eprintln!("cockpit-tui: cannot capture pane {pane}");
        std::process::exit(1);
    };
    let mut a = app::App::new(pane, cap);
    if let Some((f, last)) = resume {
        a.reopened = true;
        match app::read_state(std::path::Path::new(f), pane) {
            Ok((cmd, r)) => a.resumed(cmd, r, last),
            Err(e) => {
                eprintln!("cockpit-tui: cannot resume from {f}: {e}");
                a.say(app::Level::Error, format!("could not reopen the steps: {e}"));
            }
        }
    }
    view::run(&mut a);
    let Some(act) = a.action else { return };
    // The text needs a file beside the action line; stdout has nowhere to put it.
    let Some(f) = std::env::var_os("COCKPIT_ACTION").filter(|f| !f.is_empty()) else {
        eprintln!("cockpit-tui: {} needs COCKPIT_ACTION to carry its text; nothing was sent", act.verb);
        std::process::exit(1);
    };
    let f = std::path::Path::new(&f);
    if let Some(r) = &a.resume
        && let Err(e) = app::write_state(f, pane, &a.cap.cmd, r)
    {
        // the action still goes: only the way back to the steps is lost
        eprintln!("cockpit-tui: cannot write the steps to resume: {e}");
    }
    if let Err(e) = app::write_action(f, pane, &a.cap.cmd, &act) {
        eprintln!("cockpit-tui: cannot write the action: {e}");
        std::process::exit(1);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let action: Option<String> = match args.first().map(String::as_str) {
        Some("board") => board::run(),
        Some("transcript") => std::process::exit(data::transcript::main(&args[1..])),
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
    if let Some(a) = action {
        match std::env::var("COCKPIT_ACTION") {
            Ok(f) if !f.is_empty() => {
                let _ = std::fs::write(f, format!("{a}\n"));
            }
            _ => println!("{a}"),
        }
    }
}

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

const USAGE: &str = "cockpit-tui board | panel %N | --rows | --json | --version | -h";

/// Never publishes @cockpit_tui_pid: that pid is the board's, for its wakeups.
fn panel(pane: Option<&str>) {
    let Some(pane) = pane.filter(|p| {
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
    view::run(&mut a);
    let Some(act) = a.action else { return };
    // The text needs a file beside the action line; stdout has nowhere to put it.
    let Some(f) = std::env::var_os("COCKPIT_ACTION").filter(|f| !f.is_empty()) else {
        eprintln!("cockpit-tui: {} needs COCKPIT_ACTION to carry its text; nothing was sent", act.verb);
        std::process::exit(1);
    };
    if let Err(e) = app::write_action(std::path::Path::new(&f), pane, &a.cap.cmd, &act) {
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
            panel(args.get(1).map(String::as_str));
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

//! The CLI. Phase 0 ships the two headless forms; `board` arrives with phase 1
//! (workflow.md §3). Every mutating action stays a shell script: this process
//! prints `<verb> <pane-id>` and a wrapper runs it, so the 1000-assertion bash
//! suite keeps working with no binary present.
use cockpit_tui::{data, fleet, theme};

/// `[]` and exit 0 is "no agents"; exit 1 and no stdout is "no tmux". A wrapper
/// has to be able to tell those apart, which is what exit 1 is specified for.
fn panes() -> Vec<data::Pane> {
    match data::list_panes() {
        Some(ps) => ps,
        None => {
            eprintln!("cockpit-tui: tmux is not reachable");
            std::process::exit(1);
        }
    }
}

const USAGE: &str = "cockpit-tui --rows | --json | --version | -h";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let action: Option<String> = match args.first().map(String::as_str) {
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
    // ratatui draws to stdout, so a wrapper reading the action with $(...) would
    // swallow the whole UI and render a blank popup. COCKPIT_ACTION names a file
    // instead; without it the line still goes to stdout, for a plain shell.
    if let Some(a) = action {
        match std::env::var("COCKPIT_ACTION") {
            Ok(f) if !f.is_empty() => {
                let _ = std::fs::write(f, format!("{a}\n"));
            }
            _ => println!("{a}"),
        }
    }
}

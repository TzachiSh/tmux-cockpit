//! The CLI. Phase 0 shipped the two headless forms; phase 1 adds `board`
//! (workflow.md §3). Every mutating action stays a shell script: this process
//! prints `<verb> <pane-id>` and a wrapper runs it, so the 1000-assertion bash
//! suite keeps working with no binary present.
use cockpit_tui::{board, data, fleet, theme};

const USAGE: &str = "cockpit-tui board | --rows | --json | --version | -h";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let action: Option<String> = match args.first().map(String::as_str) {
        Some("board") => board::run(),
        Some("--rows") => {
            println!("{}", fleet::rows_text(&data::list_panes(), &theme::Theme::fixed()));
            None
        }
        Some("--json") => {
            println!("{}", fleet::rows_json(&data::list_panes()));
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

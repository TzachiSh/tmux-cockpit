//! One binary, five screens. Every action the TUI decides on is printed to
//! stdout for the shell wrapper to run (tui.md §6: the bash path stays the
//! source of truth), so this process never switches a client or kills a window.
use cockpit_tui::{ask, board, data, fleet, info, theme};

const USAGE: &str = "cockpit-tui fleet | board | info %N | ask %N | --rows | --json";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let sub = args.first().map(String::as_str).unwrap_or("fleet");
    let arg = args.get(1).map(String::as_str).unwrap_or("");
    let action = match sub {
        "--rows" => {
            println!("{}", fleet::rows_text(&data::list_panes(), &theme::Theme::fixed()));
            None
        }
        "--json" => {
            println!("{}", fleet::rows_json(&data::list_panes()));
            None
        }
        "fleet" => fleet::run(),
        "board" => board::run(),
        "info" => {
            info::run(arg);
            None
        }
        "ask" => ask::run(arg),
        "-h" | "--help" => {
            println!("{USAGE}");
            None
        }
        other => {
            eprintln!("unknown screen {other}\n{USAGE}");
            std::process::exit(2);
        }
    };
    // ratatui::init() draws to stdout, so a wrapper that reads the action with
    // $(...) would swallow the whole UI. COCKPIT_ACTION names a file instead;
    // without it the action still goes to stdout, for use from a plain shell.
    if let Some(a) = action {
        match std::env::var("COCKPIT_ACTION") {
            Ok(f) if !f.is_empty() => {
                let _ = std::fs::write(f, format!("{a}\n"));
            }
            _ => println!("{a}"),
        }
    }
}

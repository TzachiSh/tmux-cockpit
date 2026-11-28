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

const USAGE: &str = "cockpit-tui board [--select %N] [--note <file>] | pick | files <dir> [--select <name>] [--hidden] [--ignored] [--filter <text>] | branch [-C <dir>] | info [--text|--json] %N | workdir [--source] %N | pane <name> | label %N | sessions [--needs] [--current $N] | panel %N [--resume <state> --last ok|failed:<why>|hidden | --ask <file> <a> <b>] | view [--line N] [--] <file> | basket add %N <file> | ask-rules check|accept | msg send --session <id> <to> <summary-file> <body-file> | --rows | --json | --version | -h";

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Never lists a wake channel in @cockpit_board_wake: those are the board's.
fn panel(args: &[String]) {
    let (pane, start) = match cockpit_tui::panel::start_args(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("cockpit-tui: {e}\n{USAGE}");
            std::process::exit(2);
        }
    };
    let pane = pane.as_str();
    let resume = match &start {
        cockpit_tui::panel::Start::Resume(f, last) => Some((f.as_str(), last.as_str())),
        _ => None,
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
    a.live = true;
    // Explain's what-changed list (#1000): one collector for the panel's life
    let collector = cockpit_tui::panel::changes::Collector::from_env();
    a.changes = Some(cockpit_tui::panel::look::Looks::new(collector));
    a.tablet = tablet;
    a.termux = cockpit_tui::panel::is_termux();
    // other panes' hidden panels the wrapper dropped: their panes are gone
    if let Ok(gone) = std::env::var("COCKPIT_PANEL_DROPPED") {
        // counted, not named: a gone pane has no label, and its id tells a person nothing (#1407)
        match gone.split(' ').filter(|p| cockpit_tui::panel::is_pane(p)).count() {
            0 => {}
            1 => a.say(app::Level::Info, "dropped a hidden panel whose pane is gone"),
            n => a.say(app::Level::Info, format!("dropped {n} hidden panels whose panes are gone")),
        }
    }
    if let Some((f, "hidden")) = resume {
        a.reopened = true;
        a.unhide_file(std::path::Path::new(f), unix_now());
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
    // cockpit-panel says when an ask opens with a panel already hidden on the pane
    a.hide_taken = std::env::var_os("COCKPIT_PANEL_HIDE_TAKEN").is_some_and(|v| v == "1");
    // the key cockpit-panel read from the binding noted `cockpit: panel` (#1121)
    if let Ok(k) = std::env::var("COCKPIT_PANEL_KEY") {
        a.set_panel_key(&k);
    }
    // the pane's prompt basket (#365 child 5): a send from Compose carries its refs
    a.basket_raw = cockpit_tui::basket::read_text(pane).unwrap_or_default();
    a.basket = cockpit_tui::basket::from_body(&a.basket_raw);
    a.claude = !a.basket.is_empty() && cockpit_tui::basket::is_claude(pane);
    // the viewer's `?` (#365): the lines are read here, as the viewer showed them
    if let cockpit_tui::panel::Start::Ask(f, l1, l2) = &start {
        match cockpit_tui::panel::ask::load(std::path::Path::new(f), *l1, *l2) {
            Ok(q) => a.ask_open(q),
            Err(e) => {
                a.view = app::View::Error {
                    line: format!("cannot ask about it: {e}"),
                    hint: None,
                }
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
    // before the action line: cockpit-panel clears the basket only when this says
    // the send carried it, and only after the paste lands
    if let Some(b) = &a.sent_basket
        && let Err(e) = action::write_basket(f, b)
    {
        eprintln!("cockpit-tui: the basket will not be cleared after the send: {e}");
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
        // #500: the board's work dir for one pane, for tmux-review and goto
        Some("workdir") => std::process::exit(fleet::workdir_main(&args[1..])),
        // #1408: the one pane whose own @cockpit_role is the name
        Some("pane") => std::process::exit(fleet::pane_main(&args[1..])),
        // #1407: how a message names one pane, for the shell's label_ask
        Some("label") => std::process::exit(fleet::label_main(&args[1..])),
        // file browser (#363)
        Some("files") => std::process::exit(cockpit_tui::files::main(&args[1..])),
        Some("pick") => std::process::exit(cockpit_tui::pick::main(&args[1..])),
        // branch popup (#246)
        Some("branch") => std::process::exit(cockpit_tui::branch::main(&args[1..])),
        // sessions view (#245): writes its own action files, like panel
        Some("sessions") => std::process::exit(cockpit_tui::sessions::main(&args[1..])),
        // the markdown viewer (#283): prints `edit <line>` on stdout, as pick prints its key
        Some("view") => std::process::exit(cockpit_tui::mdview::main(&args[1..])),
        Some("basket") => std::process::exit(cockpit_tui::basket::main(&args[1..])),
        // the owner's auto-answer rules (#1508): check them, or accept the file as it is
        Some("ask-rules") => std::process::exit(cockpit_tui::ask::rules::main(&args[1..])),
        // one agent session's message to another (#1575)
        Some("msg") => std::process::exit(cockpit_tui::msg::main(&args[1..])),
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

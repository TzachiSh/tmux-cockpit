//! The branch popup's goto to another worktree (#303, #228): the binary picks the pane
//! working in that tree, or opens a window there, through a fake tmux that records each
//! call. Every test sets COCKPIT_TMUX, so each holds the environment lock for its body.
mod fake;

use cockpit_tui::branch::{self, Origin};
use fake::{EnvHeld, Scratch};
use std::path::Path;
use std::process::Command;

const US: char = '\u{1f}';
const WRITES: [&str; 4] = ["switch-client", "select-window", "select-pane", "new-window"];

fn git(dir: &Path, args: &[&str]) {
    let mut c = Command::new("git");
    c.arg("-C").arg(dir).args(args);
    let c = c.env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@e");
    let c = c.env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@e");
    // none of the developer's config: a gpgsign or a global hooksPath would run here
    let c = c.env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1");
    assert!(c.status().unwrap().success(), "git {args:?}");
}

/// The board's FIELDS row: id, cmd, @panestate, @cockpit_session, cwd, pid, @cockpit_dir.
fn row(id: &str, cmd: &str, state: &str, sess: &str, cwd: &str, pid: &str, opt: &str) -> String {
    fake::board_row(&[id, "t:0", "win", cmd, state, "-", "-", sess, cwd, pid, "-", "-", "@1", opt])
}

/// A repo `r` on main, its linked tree `w` holding `held`, and a fake tmux and ps.
struct Rig {
    t: Scratch,
    held: EnvHeld,
}

impl Rig {
    fn new(tag: &str) -> Rig {
        let held = fake::env_lock();
        // git lists real paths: the scratch dir is named by one, or no compare would match
        let base = std::fs::canonicalize(env!("CARGO_TARGET_TMPDIR")).unwrap();
        let t = Scratch::fresh(base.join(format!("goto-{tag}-{}", std::process::id())));
        std::fs::create_dir_all(t.join("bin")).unwrap();
        git(&t, &["init", "-q", "-b", "main", "r"]);
        git(&t.join("r"), &["commit", "-q", "--allow-empty", "-m", "i"]);
        git(
            &t.join("r"),
            &["worktree", "add", "-q", "-b", "held", t.join("w").to_str().unwrap()],
        );
        std::fs::create_dir_all(t.join("w/sub")).unwrap();
        let s = t.display();
        fake::write_exec(
            &t.join("bin/tmux"),
            &format!(
                "#!/bin/sh\n( IFS=$(printf '\\037'); printf '%s\\n' \"$*\" ) >> '{s}/calls'\n\
                 case \"$1\" in\n\
                 list-panes) cat '{s}/rows' ;;\n\
                 show-option) printf '|claude|codex|' ;;\n\
                 display-message) cat \"{s}/win$4\" 2>/dev/null || exit 1 ;;\n\
                 new-window) [ ! -e '{s}/fail' ] ;;\n\
                 esac\n"
            ),
        );
        fake::write_exec(&t.join("bin/ps"), &format!("#!/bin/sh\necho 1 >> '{s}/ps-calls'\ncat '{s}/ps'\n"));
        std::fs::write(t.join("ps"), "").unwrap();
        std::fs::write(t.join("rows"), "").unwrap();
        let path = format!("{s}/bin:{}", std::env::var("PATH").unwrap_or_default());
        // SAFETY: under the lock this rig holds for the test's life
        unsafe {
            fake::set_var(&held, "PATH", path);
            fake::set_var(&held, "COCKPIT_TMUX", format!("{s}/bin/tmux"));
            fake::set_var(&held, "HOME", s.to_string());
            fake::set_var(&held, "CLAUDE_CONFIG_DIR", format!("{s}/.claude"));
            fake::set_var(&held, "COCKPIT_PROC_DIR", format!("{s}/proc"));
            fake::remove_var(&held, "COCKPIT_READONLY");
        }
        Rig { t, held }
    }

    fn p(&self, rel: &str) -> String {
        self.t.join(rel).display().to_string()
    }

    fn rows(&self, rows: &[String]) {
        std::fs::write(self.t.join("rows"), rows.join("\n") + "\n").unwrap();
    }

    /// Every tmux call goto made, one argv each.
    fn calls(&self) -> Vec<Vec<String>> {
        let calls = std::fs::read_to_string(self.t.join("calls")).unwrap_or_default();
        calls.lines().map(|l| l.split(US).map(String::from).collect()).collect()
    }

    /// The tmux writes goto made, one argv each.
    fn writes(&self) -> Vec<Vec<String>> {
        self.calls().into_iter().filter(|a| WRITES.contains(&a[0].as_str())).collect()
    }

    fn goto(&self, path: &str, from: &Origin) -> Result<String, String> {
        let _ = std::fs::remove_file(self.t.join("calls"));
        branch::goto(&self.p("r"), path, from)
    }
}

fn sel(id: &str) -> Vec<Vec<String>> {
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    vec![
        v(&["switch-client", "-t", id]),
        v(&["select-window", "-t", id]),
        v(&["select-pane", "-t", id]),
    ]
}

/// #1248: goto's one pane scan skips cockpit's own popups, with the board's filter: its
/// `list-panes` carries `-f` and `data::NOT_POPUP`, and no other scan runs.
#[test]
fn the_pane_scan_skips_cockpits_popups() {
    let r = Rig::new("popup");
    r.rows(&[row("%2", "bash", "-", "-", &r.p("w"), "102", "")]);
    assert_eq!(r.goto(&r.p("w"), &Origin::default()), Ok(String::new()));
    let scans: Vec<Vec<String>> = r.calls().into_iter().filter(|a| a[0] == "list-panes").collect();
    assert_eq!(scans.len(), 1, "{scans:?}");
    assert_eq!(scans[0][1..4], ["-a", "-f", cockpit_tui::data::NOT_POPUP], "{scans:?}");
}

#[test]
fn a_pane_working_in_the_tree_is_selected_and_no_window_opens() {
    let r = Rig::new("sel");
    r.rows(&[
        row("%1", "bash", "-", "-", &r.p("r"), "101", ""),
        row("%2", "bash", "-", "-", &r.p("w/sub"), "102", ""),
    ]);
    assert_eq!(r.goto(&r.p("w"), &Origin::default()), Ok(String::new()));
    assert_eq!(r.writes(), sel("%2"));
}

/// Rows: an agent by command, by @panestate on a wrapper, and by `./claude` under the glob.
#[test]
fn an_agent_pane_in_the_tree_comes_before_a_shell_listed_first() {
    for (name, cmd, state) in [("claude", "claude", "-"), ("marked", "node", "running"), ("dot", "./claude", "-")] {
        let r = Rig::new(name);
        r.rows(&[
            row("%1", "bash", "-", "-", &r.p("w"), "101", ""),
            row("%3", cmd, state, "-", &r.p("w"), "103", ""),
        ]);
        assert_eq!(r.goto(&r.p("w"), &Origin::default()), Ok(String::new()), "{name}");
        assert_eq!(r.writes(), sel("%3"), "{name}");
    }
}

/// Risk (a): the script's agent test never walked the process tree. A `node` pane whose
/// tree holds claude stays a shell, so the shell listed before it is the pick.
#[test]
fn the_agent_test_walks_no_process_tree() {
    let r = Rig::new("tree");
    std::fs::write(r.t.join("ps"), "104 1 node\n105 104 claude\n").unwrap();
    r.rows(&[
        row("%6", "bash", "-", "-", &r.p("w"), "106", ""),
        row("%4", "node", "-", "-", &r.p("w"), "104", ""),
    ]);
    assert_eq!(r.goto(&r.p("w"), &Origin::default()), Ok(String::new()));
    assert_eq!(r.writes(), sel("%6"));
    assert!(!r.t.join("ps-calls").exists(), "goto forked ps");
}

/// An agent launched in main whose @cockpit_dir is the tree is the tree's, not main's.
#[test]
fn at_cockpit_dir_names_the_tree_whatever_the_cwd() {
    let r = Rig::new("opt");
    r.rows(&[
        row("%7", "claude", "-", "-", &r.p("r"), "107", &r.p("w")),
        row("%8", "bash", "-", "-", &r.p("r"), "108", ""),
    ]);
    assert_eq!(r.goto(&r.p("w"), &Origin::default()), Ok(String::new()));
    assert_eq!(r.writes(), sel("%7"), "goto w");
    assert_eq!(r.goto(&r.p("r"), &Origin::default()), Ok(String::new()));
    assert_eq!(r.writes(), sel("%8"), "goto main skips it");
}

/// Session s1's transcript, launched in main: `pad` rows of about 1 KiB with no cwd,
/// before or after one row whose cwd is the tree. Then a shell in the tree, listed
/// first, and the agent `%9` launched in main.
fn said(r: &Rig, pad: usize, cwd_last: bool) {
    let launch = r.p("r");
    let slug: String = launch
        .bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b == b'-' { b as char } else { '-' })
        .collect();
    let pj = r.t.join(".claude/projects").join(slug);
    std::fs::create_dir_all(&pj).unwrap();
    let v = serde_json::json!({"type": "user", "timestamp": "2026-08-27T00:00:00.000Z", "cwd": r.p("w"), "message": {"content": "go"}});
    let fill = format!("{}\n", serde_json::json!({"type": "x", "pad": "p".repeat(1000)})).repeat(pad);
    let body = if cwd_last { format!("{fill}{v}\n") } else { format!("{v}\n{fill}") };
    std::fs::write(pj.join("s1.jsonl"), body).unwrap();
    r.rows(&[
        row("%1", "bash", "-", "-", &r.p("w"), "101", ""),
        row("%9", "2.1.246", "running", "s1", &launch, "109", ""),
    ]);
}

/// #470: a version-named Claude launched in main whose transcript works in the tree.
#[test]
fn an_agents_transcript_cwd_names_its_tree() {
    let r = Rig::new("said");
    said(&r, 0, true);
    assert_eq!(r.goto(&r.p("w"), &Origin::default()), Ok(String::new()));
    assert_eq!(r.writes(), sel("%9"));
}

/// "YYYY-MM-DDTHH:MM:SS.000Z" `ago` seconds before now, civil-from-days (Hinnant).
fn stamp(ago: i64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
    let t = now.as_secs() as i64 - ago;
    let (days, rem) = (t.div_euclid(86400), t.rem_euclid(86400));
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// #462: an agent launched in main, whose hooks wrote @cockpit_dir = @cockpit_cwd = main,
/// ran `cd <tree>/sub && make` 30 s ago: its tool call names the tree, so goto picks it
/// over a shell in the tree listed first.
#[test]
fn an_agents_fresh_cd_into_the_tree_names_it() {
    let r = Rig::new("cd");
    let main = r.p("r");
    let slug: String = main
        .bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b == b'-' { b as char } else { '-' })
        .collect();
    let pj = r.t.join(".claude/projects").join(slug);
    std::fs::create_dir_all(&pj).unwrap();
    let call = serde_json::json!({"type": "tool_use", "id": "u1", "name": "Bash",
        "input": {"command": format!("cd {}/sub && make", r.p("w"))}});
    let v = serde_json::json!({"type": "assistant", "timestamp": stamp(30), "cwd": main, "message": {"content": [call]}});
    std::fs::write(pj.join("s1.jsonl"), format!("{v}\n")).unwrap();
    let agent = format!("{}{US}{main}", row("%9", "2.1.246", "running", "s1", &main, "109", &main));
    r.rows(&[row("%1", "bash", "-", "-", &r.p("w"), "101", ""), agent]);
    assert_eq!(r.goto(&r.p("w"), &Origin::default()), Ok(String::new()));
    assert_eq!(r.writes(), sel("%9"));
}

/// The read is the board's tail window, at most 2 MiB (TAIL_MAX in data.rs), never the
/// whole file. Rows: the cwd row before about 2.2 MB of rows without one, past the
/// window, so the agent's dir is its launch dir and the shell is the pick; and after it.
#[test]
fn a_transcript_is_read_from_its_tail_only() {
    for (name, cwd_last, want) in [("cwd past the window", false, "%1"), ("cwd at the end", true, "%9")] {
        let r = Rig::new(if cwd_last { "tail-in" } else { "tail-out" });
        said(&r, 2200, cwd_last);
        assert_eq!(r.goto(&r.p("w"), &Origin::default()), Ok(String::new()), "{name}");
        assert_eq!(r.writes(), sel(want), "{name}");
    }
}

#[test]
fn the_pinned_client_is_the_one_switched() {
    let r = Rig::new("pin");
    r.rows(&[row("%2", "bash", "-", "-", &r.p("w"), "102", "")]);
    let from = Origin {
        pane: None,
        client: Some("/dev/pts/9".into()),
    };
    assert_eq!(r.goto(&r.p("w"), &from), Ok(String::new()));
    assert_eq!(r.writes()[0], ["switch-client", "-c", "/dev/pts/9", "-t", "%2"]);
}

/// R3-8: no pane in the tree is a new window there, the path escaped as a format, beside
/// the origin's window; no other write, so no pane is cd'd.
#[test]
fn with_no_pane_there_a_window_opens_beside_the_origin() {
    let r = Rig::new("new");
    let w = r.p("wt #(touch F) a#[b]c");
    git(&r.t.join("r"), &["worktree", "add", "-q", "-b", "odd", &w]);
    std::fs::write(r.t.join("win%1"), "@5\n").unwrap();
    r.rows(&[row("%1", "bash", "-", "-", &r.p("r"), "101", "")]);
    let from = Origin {
        pane: Some("%1".into()),
        client: None,
    };
    assert_eq!(r.goto(&w, &from), Ok(String::new()));
    let at = format!("{}/wt ##(touch F) a#[b]c", r.t.display());
    assert_eq!(r.writes(), [["new-window", "-a", "-t", "@5", "-c", at.as_str()]]);
}

/// Rows: the origin pane is gone (tmux answers nothing), and none was given.
#[test]
fn an_origin_gone_or_absent_still_gets_a_window() {
    for (name, pane) in [("gone", Some("%99".to_string())), ("none", None)] {
        let r = Rig::new(name);
        let from = Origin { pane, client: None };
        assert_eq!(r.goto(&r.p("w"), &from), Ok(String::new()), "{name}");
        assert_eq!(r.writes(), [["new-window", "-c", r.p("w").as_str()]], "{name}");
    }
}

/// Rows: a dir that is no worktree, a relative path, and an empty one: refused by name,
/// with no tmux write.
#[test]
fn a_path_git_does_not_list_is_refused_and_writes_nothing() {
    let r = Rig::new("refuse");
    r.rows(&[row("%2", "bash", "-", "-", &r.p("w"), "102", "")]);
    for p in [r.p("bin"), "relative/path".into(), String::new()] {
        let got = r.goto(&p, &Origin::default());
        assert_eq!(got, Err(format!("cockpit: not a worktree of this repository: {p}")), "{p:?}");
        assert_eq!(r.writes(), Vec::<Vec<String>>::new(), "{p:?}");
    }
}

#[test]
fn a_listed_tree_that_is_gone_is_refused_by_name() {
    let r = Rig::new("gone");
    let g = r.p("g");
    git(&r.t.join("r"), &["worktree", "add", "-q", "-b", "g", &g]);
    std::fs::remove_dir_all(&g).unwrap();
    assert_eq!(
        r.goto(&g, &Origin::default()),
        Err(format!("cockpit: {g} is gone · git worktree prune"))
    );
    assert_eq!(r.writes(), Vec::<Vec<String>>::new());
}

#[test]
fn a_window_tmux_refuses_is_named() {
    let r = Rig::new("fail");
    std::fs::write(r.t.join("fail"), "").unwrap();
    let w = r.p("w");
    assert_eq!(
        r.goto(&w, &Origin::default()),
        Err(format!("cockpit: could not open a window in {w}"))
    );
}

#[test]
fn readonly_writes_nothing() {
    let r = Rig::new("ro");
    r.rows(&[row("%2", "bash", "-", "-", &r.p("w"), "102", "")]);
    // SAFETY: under the rig's lock
    unsafe { fake::set_var(&r.held, "COCKPIT_READONLY", "1") };
    assert!(r.goto(&r.p("w"), &Origin::default()).is_err());
    assert_eq!(r.writes(), Vec::<Vec<String>>::new());
}

/// Rows: a nested tree under main, listed after main and before it; a sibling whose name
/// starts with the tree's; the tree itself.
#[test]
fn the_deepest_tree_holding_a_dir_owns_it() {
    let t = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(branch::owner_of("/r/n/x", &t(&["/r", "/r/n"])), Some("/r/n"), "nested after");
    assert_eq!(branch::owner_of("/r/n/x", &t(&["/r/n", "/r"])), Some("/r/n"), "nested before");
    assert_eq!(branch::owner_of("/r2/x", &t(&["/r"])), None, "a sibling with the prefix");
    assert_eq!(branch::owner_of("/r", &t(&["/r"])), Some("/r"), "the tree itself");
}

#[test]
fn fesc_doubles_a_hash_but_a_run_before_a_bracket() {
    for (s, want) in [
        ("a#b", "a##b"),
        ("#(x)", "##(x)"),
        ("a#[b]", "a#[b]"),
        ("a##[b", "a##[b"),
        ("x#", "x##"),
        ("", ""),
    ] {
        assert_eq!(branch::fesc(s), want, "{s:?}");
    }
}

/// Rows: a pane id and a client kept, then each shape that is none.
#[test]
fn origin_reads_only_a_pane_id_and_a_printable_client() {
    let held = fake::env_lock();
    for (pane, client, want) in [
        (
            "%3",
            "/dev/pts/2",
            Origin {
                pane: Some("%3".into()),
                client: Some("/dev/pts/2".into()),
            },
        ),
        ("3", "", Origin::default()),
        ("%3x", "a\nb", Origin::default()),
    ] {
        // SAFETY: under the lock this test holds
        unsafe {
            fake::set_var(&held, "COCKPIT_BRANCH_PANE", pane);
            fake::set_var(&held, "COCKPIT_BRANCH_CLIENT", client);
        }
        assert_eq!(Origin::from_env(), want, "{pane:?} {client:?}");
    }
}

/// git before 2.36 rejects `worktree list -z`, as a shim here does: the plain porcelain
/// still names w, whose pane is selected, and a path with a newline, cut in two there,
/// is no worktree (the #303 rows on the old-binary path).
#[test]
fn without_worktree_list_z_the_tree_is_found_and_a_newline_path_refused() {
    let r = Rig::new("oldgit");
    git(&r.t.join("r"), &["worktree", "add", "-q", "-b", "nl", &r.p("nl\nx")]);
    let real = Command::new("sh").args(["-c", "command -v git"]).output().unwrap().stdout;
    let real = String::from_utf8(real).unwrap();
    std::fs::create_dir_all(r.t.join("old")).unwrap();
    let shim = "#!/bin/sh\ncase \" $* \" in *\" worktree list \"*\" -z \"*) exit 129 ;; esac\n";
    fake::write_exec(&r.t.join("old/git"), &format!("{shim}exec '{}' \"$@\"\n", real.trim()));
    let mut z = Command::new(r.t.join("old/git"));
    z.args(["-C", &r.p("r"), "worktree", "list", "--porcelain", "-z"]);
    z.env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1");
    assert_eq!(z.status().unwrap().code(), Some(129), "the shim refuses -z");
    let path = format!("{}:{}", r.p("old"), std::env::var("PATH").unwrap_or_default());
    // SAFETY: under the rig's lock
    unsafe { fake::set_var(&r.held, "PATH", path) };
    r.rows(&[row("%2", "bash", "-", "-", &r.p("w"), "102", "")]);
    assert_eq!(r.goto(&r.p("w"), &Origin::default()), Ok(String::new()));
    assert_eq!(r.writes(), sel("%2"));
    let cut = r.p("nl");
    assert_eq!(
        r.goto(&cut, &Origin::default()),
        Err(format!("cockpit: not a worktree of this repository: {cut}"))
    );
}

/// #1374: a row one field short of FIELDS is not the board's format: goto skips its pane,
/// an agent's too, and selects the shell in the tree.
#[test]
fn a_row_one_field_short_is_no_pane_to_goto() {
    let r = Rig::new("short");
    let agent = row("%3", "claude", "-", "-", &r.p("w"), "103", "");
    let short = agent.rsplit_once('\u{1f}').unwrap().0.to_string();
    r.rows(&[row("%2", "bash", "-", "-", &r.p("w"), "102", ""), short]);
    assert_eq!(r.goto(&r.p("w"), &Origin::default()), Ok(String::new()));
    assert_eq!(r.writes(), sel("%2"));
}

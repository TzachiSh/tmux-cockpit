//! What the environment drives: the child's env, `Call::from_env`, overrides and
//! the capture. Env vars are process-global, so every test holds `ENV`.
mod fake;

use cockpit_tui::panel::capture::{capture, capture_within, repo_state_within};
use cockpit_tui::panel::envelope::{Flow, OutType};
use cockpit_tui::panel::prompts::{self, Ctx};
use cockpit_tui::panel::request::{Call, Effort, Request};
use cockpit_tui::panel::route::route;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

static ENV: Mutex<()> = Mutex::new(());

/// What a row checking the repo block's parts waits for it. The panel's 300 ms
/// `REPO_BUDGET` leaves a part out on a loaded box (#596).
const BUDGET: Duration = Duration::from_secs(10);
/// The time-out rows' budget. Their hung part runs to it every time, so it stays
/// short; 2 s, about 7 times the panel's, gives the parts that do answer, and a
/// fake git's shell start-up, room on most boxes; [`growing`] doubles it for a box
/// loaded past that.
const HUNG_BUDGET: Duration = Duration::from_secs(2);

struct Env {
    old: Vec<(&'static str, Option<String>)>,
    dir: PathBuf,
    _lock: MutexGuard<'static, ()>,
}

impl Env {
    /// `None` unsets the variable for the test. The dir's name holds a space, as a
    /// TMPDIR with one would give it, so a `$D` left unquoted in a fake's body
    /// fails the row that runs that fake (#757).
    fn new(tag: &str, vars: &[(&'static str, Option<String>)]) -> Env {
        let lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("cockpit panel-env-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let old = vars.iter().map(|(k, _)| (*k, std::env::var(k).ok())).collect();
        for (k, v) in vars {
            // SAFETY: ENV is held for the guard's lifetime and every test here takes it.
            unsafe {
                match v {
                    Some(v) => std::env::set_var(k, v.replace("$D", &dir.display().to_string())),
                    None => std::env::remove_var(k),
                }
            }
        }
        Env { old, dir, _lock: lock }
    }

    fn exec(&self, name: &str, body: &str) -> String {
        let p = self.dir.join(name);
        fake::write_exec(&p, &format!("#!/bin/sh\n{}\n", body.replace("$D", &self.dir.display().to_string())));
        p.display().to_string()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        for (k, v) in &self.old {
            // SAFETY: as above.
            unsafe {
                match v {
                    Some(v) => std::env::set_var(k, v),
                    None => std::env::remove_var(k),
                }
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The space in [`Env::new`]'s dir is what makes the other rows check their
/// fakes' quoting: without it they pass with `$D` unquoted.
#[test]
fn every_env_dir_name_holds_a_space() {
    let e = Env::new("space", &[]);
    let name = e.dir.file_name().unwrap().to_string_lossy();
    assert!(name.contains(' '), "{}", e.dir.display());
}

fn until(mut f: impl FnMut() -> bool) -> bool {
    for _ in 0..1000 {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn the_call_is_no_panes_agent_and_marks_itself() {
    let e = Env::new(
        "child",
        &[
            ("CLAUDECODE", Some("1".into())),
            ("TMUX", Some("/tmp/tmux-1000/default,1,0".into())),
            ("TMUX_PANE", Some("%3".into())),
            ("COCKPIT_PANEL_CALL", None),
            ("COCKPIT_CLAUDE", Some("$D/claude".into())),
            ("COCKPIT_PANEL_TIMEOUT", Some("7".into())),
            ("XDG_CACHE_HOME", Some("$D/cache".into())),
        ],
    );
    e.exec("claude", "env > \"$D/env\"\necho ok");
    let call = Call::from_env();
    assert_eq!(call.claude, format!("{}/claude", e.dir.display()));
    assert_eq!(call.timeout, Duration::from_secs(7));
    assert_eq!(call.dir, e.dir.join("cache/tmux-cockpit/panel"));

    let mut req = Request::spawn(&call, "S", "p", "haiku");
    let mut got = None;
    assert!(until(|| {
        got = req.poll();
        got.is_some()
    }));
    assert_eq!(got.unwrap(), Ok("ok\n".into()));
    let env = std::fs::read_to_string(e.dir.join("env")).unwrap();
    let lines: Vec<&str> = env.lines().collect();
    assert!(lines.contains(&"COCKPIT_PANEL_CALL=1"), "{env}");
    for k in ["CLAUDECODE=", "TMUX=", "TMUX_PANE="] {
        assert!(!lines.iter().any(|l| l.starts_with(k)), "{k} leaked into the child:\n{env}");
    }
}

#[test]
fn the_call_defaults_without_its_variables() {
    let e = Env::new(
        "defaults",
        &[
            ("COCKPIT_CLAUDE", None),
            ("COCKPIT_PANEL_TIMEOUT", Some("soon".into())),
            ("XDG_CACHE_HOME", Some("relative/cache".into())),
            ("HOME", Some("$D".into())),
        ],
    );
    let call = Call::from_env();
    assert_eq!(call.claude, "claude");
    assert_eq!(call.timeout, Duration::from_secs(120), "an unparsable timeout is the default");
    assert_eq!(
        call.dir,
        e.dir.join(".cache/tmux-cockpit/panel"),
        "a relative XDG_CACHE_HOME is ignored"
    );
}

#[test]
fn cockpit_panel_model_beats_every_choice() {
    let _e = Env::new("model", &[("COCKPIT_PANEL_MODEL", Some("opus".into()))]);
    let r = route(
        &Flow::Write {
            draft: "x".into(),
            out: OutType::Steps,
        },
        "haiku",
    );
    assert_eq!(
        (r.model.as_str(), r.why.as_str(), r.suggest),
        ("opus", "COCKPIT_PANEL_MODEL is set", None)
    );
}

#[test]
fn the_shipped_prompts_render_without_a_placeholder_left() {
    let _e = Env::new(
        "shipped",
        &[("XDG_CONFIG_HOME", Some("$D/cfg".into())), ("HOME", Some("/home/u".into()))],
    );
    let platform = prompts::platform();
    let write = prompts::write(&Ctx {
        window: "shell",
        ..Ctx::default()
    });
    for (name, text) in [("write", &write), ("explain", &prompts::explain()), ("fix", &prompts::fix())] {
        assert!(!text.contains("{{"), "{name} left a placeholder");
        assert!(text.contains(&platform), "{name} has no platform");
        assert!(!text.ends_with('\n'), "{name} keeps its trailing newline");
    }
    assert!(
        write.contains("<examples>\n<example>") && write.contains("@/home/u/bin/net-proxy"),
        "examples carry {{home}}"
    );
}

/// HOME is the test's own: the examples carry `{{home}}`, so a home folder whose
/// name holds a tag would answer the searches below (#910). Each prompt is first
/// checked to hold that HOME, so the text searched is the one rendered from it.
#[test]
fn the_shipped_write_prompt_never_asks_for_a_file_edit() {
    let vars = [("XDG_CONFIG_HOME", Some("$D/cfg".into())), ("HOME", Some("/home/u".into()))];
    let _e = Env::new("no-edit", &vars);
    for out in [OutType::Auto, OutType::Command, OutType::Format, OutType::Hint] {
        for window in ["claude", "shell", "ssh", "other"] {
            let w = prompts::write(&Ctx {
                out,
                window,
                ..Ctx::default()
            });
            assert!(w.contains("@/home/u/bin/net-proxy"), "{out:?} {window}: not this HOME's");
            for tag in ["<edit", "<need_file", "<file"] {
                assert!(!w.contains(tag), "{out:?} {window}: {tag}");
            }
        }
    }
}

#[test]
fn a_users_prompt_file_wins_over_the_shipped_one() {
    let e = Env::new("override", &[("XDG_CONFIG_HOME", Some("$D/cfg".into()))]);
    let d = e.dir.join("cfg/tmux-cockpit/prompts");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("fix.txt"), "Mine on {{platform}}\n\n").unwrap();
    std::fs::write(d.join("write.examples.txt"), "<example id=\"x\" core=\"1\">only mine</example>").unwrap();
    assert_eq!(prompts::fix(), format!("Mine on {}", prompts::platform()));
    assert!(
        prompts::explain().starts_with("You help a user understand"),
        "an absent override keeps the shipped text"
    );
    let write = prompts::write(&Ctx::default());
    assert!(write.ends_with("<examples>\n<example>only mine</example>\n</examples>"), "{write}");
}

#[test]
fn termux_is_named_by_either_variable() {
    let termux = |v: Option<&str>, p: Option<&str>| {
        let _e = Env::new(
            "termux",
            &[("TERMUX_VERSION", v.map(String::from)), ("PREFIX", p.map(String::from))],
        );
        (cockpit_tui::panel::is_termux(), prompts::platform())
    };
    for (v, p) in [(Some("0.118.0"), None), (None, Some("/data/data/com.termux/files/usr"))] {
        let (on, platform) = termux(v, p);
        assert!(on, "{v:?} {p:?}");
        assert!(
            platform.starts_with("an Android tablet or phone running Termux (tmux;"),
            "{platform}"
        );
    }
    let (on, platform) = termux(Some(""), Some("/usr"));
    assert!(!on);
    assert!(!platform.contains("Termux"), "{platform}");
}

/// The whole note for an override at `p` that exists and cannot be read. `p` is
/// there in full, so the file's name is compared at its place in the path and a
/// temp folder of that name cannot stand in for it (#910). The error is the one
/// the test's own read of `p` gives, whichever way the file was made unreadable.
fn unread(p: &Path) -> String {
    let e = std::fs::read_to_string(p).expect_err("the test can read the override");
    format!("cannot read {}: {e}; using the shipped prompt", p.display())
}

#[test]
fn a_broken_prompt_file_falls_back_and_says_why() {
    let e = Env::new("broken", &[("XDG_CONFIG_HOME", Some("$D/cfg".into()))]);
    let d = e.dir.join("cfg/tmux-cockpit/prompts");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("fix.txt"), b"bad \xff byte").unwrap();
    let (fix, note) = prompts::fix_noted();
    assert!(
        fix.starts_with("You correct a draft"),
        "invalid UTF-8 keeps the shipped text: {fix}"
    );
    assert_eq!(note, Some(unread(&d.join("fix.txt"))));

    let p = d.join("explain.txt");
    std::fs::write(&p, "Mine").unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000)).unwrap();
    // Root reads a mode-000 file anyway; a directory fails for everyone.
    if std::fs::read(&p).is_ok() {
        std::fs::remove_file(&p).unwrap();
        std::fs::create_dir(&p).unwrap();
    }
    let (explain, note) = prompts::explain_noted();
    assert!(explain.starts_with("You help a user understand"), "{explain}");
    assert_eq!(note, Some(unread(&p)));
    assert_eq!(prompts::write_noted(&Ctx::default()).1, None, "an absent override is no note");
}

fn fake_tmux(e: &Env, display: &str) {
    let body = format!(
        ": > \"$D/ran\"\ncase \"$1\" in\ndisplay) printf '{display}' ;;\n\
         capture-pane) echo \"$@\" > \"$D/args\"; printf 'line1\\n\\nline2\\n  \\n\\n' ;;\n\
         *) exit 1 ;;\nesac"
    );
    e.exec("tmux", &body);
}

/// The vars for a row with a fake tmux: COCKPIT_TMUX is the bare name, the
/// production default, found first on a PATH that starts with `$D`. The variable
/// itself is an argv split on whitespace (`data::tmux_argv`), so it cannot hold
/// the path of a fake in a dir with a space. With no TMUX and TMUX_TMPDIR in `$D`,
/// a real tmux reached by mistake finds no server, never the user's. [`Env`]
/// restores all four on drop.
fn tmux_on_path() -> [(&'static str, Option<String>); 4] {
    let path = format!("$D:{}", std::env::var("PATH").unwrap_or_default());
    [
        ("COCKPIT_TMUX", Some("tmux".into())),
        ("PATH", Some(path)),
        ("TMUX", None),
        ("TMUX_TMPDIR", Some("$D".into())),
    ]
}

/// Whether [`fake_tmux`]'s fake ran since the last ask; its marker is taken, so
/// the next ask in a loop starts fresh. A row whose asserts a real tmux could
/// also meet (a block that is "") checks it.
fn ran(e: &Env) -> bool {
    std::fs::remove_file(e.dir.join("ran")).is_ok()
}

#[test]
fn a_claude_pane_is_captured_deeper_and_trimmed() {
    let e = Env::new("capture", &tmux_on_path());
    fake_tmux(&e, "claude\\tclaude\\t/w\\n");
    let c = capture("%5").unwrap();
    assert_eq!((c.name.as_str(), c.cmd.as_str(), c.path.as_str()), ("claude", "claude", "/w"));
    assert_eq!(c.screen, "line1\n\nline2");
    assert_eq!(
        std::fs::read_to_string(e.dir.join("args")).unwrap().trim(),
        "capture-pane -p -J -t %5 -S -200"
    );
}

#[test]
fn a_shell_pane_with_no_window_name_keeps_its_fields() {
    let e = Env::new("capture-shell", &tmux_on_path());
    fake_tmux(&e, "\\tbash\\t/home/u\\n");
    let c = capture("%6").unwrap();
    assert_eq!((c.name.as_str(), c.cmd.as_str(), c.path.as_str()), ("", "bash", "/home/u"));
    assert!(std::fs::read_to_string(e.dir.join("args")).unwrap().contains("-S -80"));
}

/// `n` threads outside ENV that fork until the storm is stopped or dropped, as capture()'s
/// repo worker does after its test returns (#544 moved it out of the test).
struct ForkStorm {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl ForkStorm {
    fn start(n: usize) -> Self {
        use std::os::unix::process::CommandExt;
        use std::sync::atomic::{AtomicBool, Ordering};
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let threads = (0..n)
            .map(|_| {
                let stop = stop.clone();
                std::thread::spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        let mut c = std::process::Command::new("true");
                        // SAFETY: the closure does nothing; it only makes std fork,
                        // not posix_spawn.
                        unsafe { c.pre_exec(|| Ok(())) };
                        let _ = c.status();
                    }
                })
            })
            .collect();
        ForkStorm { stop, threads }
    }

    /// The storm's end, said where a test means it; the work is in Drop.
    fn stop(self) {}
}

/// Stops and joins the threads on every exit path, a failed assert's unwind
/// included, so a panicking test does not leave forks running (review-510).
impl Drop for ForkStorm {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        for t in self.threads.drain(..) {
            let joined = t.join();
            // a storm thread's own panic fails the test, unless one is unwinding
            if !std::thread::panicking() {
                joined.unwrap();
            }
        }
    }
}

/// A test that panics before [`ForkStorm::stop`] still stops its storm: each
/// thread holds a clone of `stop`, so the count is back to 1 once all are joined.
#[test]
fn a_panic_before_stop_still_ends_the_storm() {
    let storm = ForkStorm::start(4);
    let stop = storm.stop.clone();
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _storm = storm;
        panic!("an assert failed");
    }));
    assert!(unwound.is_err());
    assert_eq!(std::sync::Arc::strong_count(&stop), 1, "a storm thread outlived the panic");
}

/// The child of a fork holds a copy of every fd the parent had open, a fake
/// being written among them, until it execs; the fake's own exec meanwhile is
/// ETXTBSY. 200 rewrites of one fake, each run at once, under four threads of
/// such forks.
#[test]
fn a_fresh_fake_execs_while_another_thread_forks() {
    let e = Env::new("busy", &[]);
    let storm = ForkStorm::start(4);
    let busy: Vec<String> = (0..200)
        .filter_map(|i| {
            let p = e.exec("fake", &format!("exit {}", i % 2));
            std::process::Command::new(&p).status().err().map(|err| format!("{i}: {err}"))
        })
        .collect();
    storm.stop();
    assert_eq!(busy, Vec::<String>::new(), "these execs failed");
}

#[test]
fn a_dead_pane_captures_nothing() {
    let e = Env::new("capture-dead", &tmux_on_path());
    e.exec("tmux", "touch \"$D/ran\"; exit 1");
    assert_eq!(capture("%7"), None);
    // a tmux that never started is None too: the marker tells the two apart
    assert!(e.dir.join("ran").exists(), "tmux never started");
    assert!(!Path::new(&e.dir.join("args")).exists());
}

#[test]
fn an_override_that_cannot_be_read_is_a_warn_note_on_the_busy_view() {
    use cockpit_tui::panel::app::{App, Level, View};
    use cockpit_tui::panel::capture::Capture;
    let e = Env::new("noted", &[("XDG_CONFIG_HOME", Some("$D/cfg".into()))]);
    // a directory where the file should be: it exists, and reading it fails
    let p = e.dir.join("cfg/tmux-cockpit/prompts/explain.txt");
    std::fs::create_dir_all(&p).unwrap();
    let call = Call {
        claude: e.exec("claude", "cat >/dev/null; exec sleep 1000"),
        timeout: Duration::from_secs(77),
        dir: e.dir.join("cache"),
        effort: Effort::Medium,
    };
    let mut a = App::new("%3", Capture::default());
    a.view = View::ExplainAsk { sel: 0 };
    a.key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Enter));
    a.launch(&call);
    let n = a.note.clone().expect("a note");
    assert_eq!(n.level, Level::Warn);
    assert_eq!(n.text, unread(&p));
    assert_eq!(a.timeout, Duration::from_secs(77), "the limit comes from the call");
    a.stop();
}

fn git_init(dir: &Path) {
    let ok = std::process::Command::new("git")
        .args(["-c", "init.defaultBranch=main", "init", "-q"])
        .arg(dir)
        .status()
        .unwrap();
    assert!(ok.success());
}

#[test]
fn a_shell_pane_in_a_repo_carries_its_repo_block() {
    let e = Env::new("capture-repo", &tmux_on_path());
    git_init(&e.dir.join("r"));
    fake_tmux(&e, "w\\tbash\\t$D/r\\n");
    let c = capture_within("%8", BUDGET).unwrap();
    assert!(c.repo.get().starts_with("branch: main (no commits yet)"), "{:?}", c.repo);
}

#[test]
fn an_ssh_pane_or_no_path_carries_no_repo_block() {
    let e = Env::new("capture-ssh", &tmux_on_path());
    git_init(&e.dir.join("r"));
    // rows: the window line tmux prints
    for display in ["w\\tssh\\t$D/r\\n", "w\\tmosh-client\\t$D/r\\n", "w\\tbash\\t\\n"] {
        fake_tmux(&e, display);
        let c = capture("%9").unwrap();
        assert!(ran(&e), "{display}: tmux never started");
        assert_eq!(c.repo.get(), "", "{display}");
    }
}

#[test]
fn a_hung_git_is_killed_with_what_it_forked_at_the_budget() {
    let path = format!("$D:{}", std::env::var("PATH").unwrap_or_default());
    let e = Env::new("repo-hung", &[("PATH", Some(path))]);
    // every call hangs: the rev-parse started beside the config read must die too.
    // Each try has its own repo dir, so a late git from a missed try writes its pid
    // to that try's file, never to the next one's (#727).
    e.exec(
        "git",
        "case \"$2\" in \"$D\"/*) ;; *) exit 1 ;; esac; sleep 1000 & echo $! >> \"$2/kids\"; wait",
    );
    let tries = std::cell::Cell::new(0);
    let run = |b| {
        let r = e.dir.join(tries.replace(tries.get() + 1).to_string());
        std::fs::create_dir(&r).unwrap();
        let got = repo_state_within(&r.display().to_string(), "", b);
        (got, std::fs::read_to_string(r.join("kids")).unwrap_or_default())
    };
    // the kill comes at the budget: rev-parse and config must have written their pid by then
    let both = |(got, kids): &(String, String)| got.is_empty() && kids.lines().count() == 2;
    let got = growing(HUNG_BUDGET, run, both);
    // every try's pids, the missed ones too. Each poll reads the files again, and the
    // first poll that finds every listed pid dead ends the check: a pid written after
    // that poll is not checked.
    let kids = || (0..tries.get()).map(|t| std::fs::read_to_string(e.dir.join(format!("{t}/kids"))).unwrap_or_default());
    // gone, or a zombie its new parent has not reaped yet
    let dead = |k: &str| std::fs::read_to_string(format!("/proc/{k}/stat")).map_or(true, |s| s.contains(") Z "));
    assert!(
        until(|| kids().collect::<String>().lines().all(dead)),
        "{:?} lives",
        kids().collect::<Vec<_>>()
    );
    assert!(got.is_ok(), "every budget missed: {got:?}");
}

/// A fake git on PATH: `body` runs for `sub`, the other subcommands answer as a
/// repo on `main` with one commit.
fn fake_git(e: &Env, sub: &str, body: &str) {
    e.exec(
        "git",
        &format!(
            "[ \"$2\" = \"$D\" ] || exit 1\n\
             for a; do case $a in config|rev-parse|status|log|diff) s=$a; break ;; esac; done\n\
             if [ \"$s\" = {sub} ]; then {body}; fi\n\
             case $s in\nconfig) exit 1 ;;\nrev-parse) echo /r ;;\nstatus) echo '## main' ;;\n\
             log) echo 'abc1234 first' ;;\nesac"
        ),
    );
}

#[test]
fn a_status_that_times_out_keeps_the_log() {
    let path = format!("$D:{}", std::env::var("PATH").unwrap_or_default());
    let e = Env::new("repo-status-hung", &[("PATH", Some(path))]);
    fake_git(&e, "status", "exec sleep 1000");
    let dir = e.dir.display().to_string();
    let kept = |got: &String| got.contains("status: timed out") && got.contains("abc1234 first");
    let got = growing(HUNG_BUDGET, |b| repo_state_within(&dir, "", b), kept);
    assert!(got.is_ok(), "every budget missed: {got:?}");
}

/// The first of `run`'s results that `ok` takes, from `first` and twice and four
/// times it, or all three misses. A hung part costs each try its whole budget, so
/// one `first` is the usual cost; a box too loaded for the parts that do answer
/// to answer within it gets the larger ones before the row fails (#717).
fn growing<T>(first: Duration, run: impl Fn(Duration) -> T, ok: impl Fn(&T) -> bool) -> Result<T, Vec<T>> {
    let mut misses = Vec::new();
    for budget in [first, first * 2, first * 4] {
        let got = run(budget);
        if ok(&got) {
            return Ok(got);
        }
        misses.push(got);
    }
    Err(misses)
}

/// What an inherited environment could point git at instead of the pane's repo.
const REDIRECT: [&str; 4] = ["GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR", "GIT_INDEX_FILE"];

/// git in `dir` as the test's own setup, none of [`REDIRECT`] inherited.
fn setup_git(dir: &Path, args: &[&str]) {
    let mut c = std::process::Command::new("git");
    REDIRECT.iter().for_each(|v| _ = c.env_remove(v));
    let id = ["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"];
    let ok = c.arg("-C").arg(dir).args(id).args(args).status().unwrap();
    assert!(ok.success(), "git {args:?}");
}

/// `$D/pane` on main and `$D/other` on elsewhere, one different commit each.
fn pane_and_other(e: &Env) -> String {
    for (name, branch, file) in [("pane", "main", "a"), ("other", "elsewhere", "b")] {
        let d = e.dir.join(name);
        std::fs::create_dir_all(&d).unwrap();
        setup_git(&d, &["init", "-q", "-b", branch]);
        std::fs::write(d.join(file), "x\n").unwrap();
        setup_git(&d, &["add", "-A"]);
        setup_git(&d, &["commit", "-q", "-m", &format!("{name} commit")]);
    }
    e.dir.join("pane").display().to_string()
}

/// The panel may start with a dotfiles setup's GIT_DIR, or one of its kin, in
/// its environment: the repo block still describes the pane's own repo.
#[test]
fn an_inherited_git_variable_never_moves_the_repo_block() {
    let rows = [
        ("GIT_DIR", "$D/other/.git"),
        ("GIT_WORK_TREE", "$D/other"),
        ("GIT_COMMON_DIR", "$D/other/.git"),
        ("GIT_INDEX_FILE", "$D/other/.git/index"),
    ];
    let moved: Vec<&str> = rows
        .into_iter()
        .filter(|(var, val)| {
            let mut vars: Vec<_> = REDIRECT.iter().map(|v| (*v, None)).collect();
            vars.extend([
                ("GIT_CONFIG_GLOBAL", Some("/dev/null".into())),
                ("GIT_CONFIG_NOSYSTEM", Some("1".into())),
            ]);
            let e = Env::new("repo-redirect", &vars);
            let pane = pane_and_other(&e);
            let block = |p: &str| repo_state_within(p, "", BUDGET);
            let want = block(&pane);
            assert!(want.starts_with(&format!("branch: main\nroot: {pane}\n")), "{want}");
            // SAFETY: `e` holds ENV, and its drop puts every REDIRECT var back.
            unsafe { std::env::set_var(var, val.replace("$D", &e.dir.display().to_string())) };
            block(&pane) != want
        })
        .map(|(var, _)| var)
        .collect();
    assert_eq!(moved, Vec::<&str>::new(), "these moved the block");
}

#[test]
fn capture_returns_before_git_answers() {
    let e = Env::new("repo-lazy", &tmux_on_path());
    // git waits for a file the test writes only once capture() is back. `until`, not
    // `while !`: a `[` that errors, as on an unquoted $D in a spaced dir, keeps it
    // waiting, so the config read misses BUDGET and the block is "", where a
    // `while` would end at once and the row would pass without a wait.
    fake_git(&e, "config", "until [ -e \"$D/go\" ]; do sleep 0.01; done");
    fake_tmux(&e, "w\\tbash\\t$D\\n");
    let c = capture_within("%4", BUDGET).unwrap();
    std::fs::write(e.dir.join("go"), "").unwrap();
    assert!(c.repo.get().starts_with("branch: main"), "{:?}", c.repo);
}

// --- #429: the repo block goes only where the shell runs here ---

#[test]
fn a_container_or_unknown_command_pane_carries_no_repo_block() {
    let e = Env::new("capture-container", &tmux_on_path());
    git_init(&e.dir.join("r"));
    // rows: pane_current_command, a folder that is only the local client's;
    // toolbox stands for any command not on the list, and "" for none at all
    for cmd in ["docker", "podman", "kubectl", "lxc", "incus", "nerdctl", "toolbox", ""] {
        fake_tmux(&e, &format!("w\\t{cmd}\\t$D/r\\n"));
        let c = capture("%9").unwrap();
        assert!(ran(&e), "{cmd:?}: tmux never started");
        assert_eq!(c.repo.get(), "", "{cmd:?}");
    }
}

#[test]
fn a_local_shell_editor_or_agent_pane_keeps_its_repo_block() {
    let e = Env::new("capture-local", &tmux_on_path());
    git_init(&e.dir.join("r"));
    // rows: pane_current_command; 2.1.246 is a live Claude renamed to its version
    for cmd in ["zsh", "fish", "nvim", "vim", "claude", "codex", "2.1.246", "cursor-agent"] {
        fake_tmux(&e, &format!("w\\t{cmd}\\t$D/r\\n"));
        let c = capture_within("%9", BUDGET).unwrap();
        assert!(c.repo.get().starts_with("branch: main (no commits yet)"), "{cmd}: {:?}", c.repo);
    }
}

#[test]
fn every_ssh_like_command_carries_no_repo_block() {
    let e = Env::new("capture-ssh-like", &tmux_on_path());
    git_init(&e.dir.join("r"));
    for cmd in cockpit_tui::panel::capture::SSH_LIKE {
        fake_tmux(&e, &format!("w\\t{cmd}\\t$D/r\\n"));
        let c = capture("%9").unwrap();
        assert!(ran(&e), "{cmd}: tmux never started");
        assert_eq!(c.repo.get(), "", "{cmd}");
    }
}

/// The word after `--effort` in an argv written one per line.
fn effort_in(argv: &str) -> Option<&str> {
    let mut lines = argv.lines();
    lines.position(|l| l == "--effort")?;
    lines.next()
}

/// COCKPIT_PANEL_EFFORT (`@cockpit_panel_effort`, passed by cockpit-panel) is
/// the call's `--effort`.
#[test]
fn the_panel_effort_is_the_calls_effort() {
    let e = Env::new(
        "effort",
        &[
            ("COCKPIT_PANEL_EFFORT", Some("low".into())),
            ("COCKPIT_CLAUDE", Some("$D/claude".into())),
            ("XDG_CACHE_HOME", Some("$D/cache".into())),
        ],
    );
    e.exec("claude", "printf '%s\\n' \"$@\" > \"$D/argv\"\necho ok");
    let call = Call::from_env();
    assert_eq!(call.effort, Effort::Low);
    let mut req = Request::spawn(&call, "S", "p", "haiku");
    assert!(until(|| req.poll().is_some()));
    let argv = std::fs::read_to_string(e.dir.join("argv")).unwrap();
    assert_eq!(effort_in(&argv), Some("low"), "{argv}");
}

/// Unset, empty or not one of the CLI's words, the effort is `medium`.
#[test]
fn a_missing_or_unknown_panel_effort_is_medium() {
    let rows = [("unset", None), ("empty", Some("")), ("shell", Some("max$(touch x)"))];
    for (row, v) in rows {
        let _e = Env::new("effort-bad", &[("COCKPIT_PANEL_EFFORT", v.map(String::from))]);
        assert_eq!(Call::from_env().effort, Effort::Medium, "{row}");
    }
}

/// claude's CLAUDE_CODE_EFFORT_LEVEL beats `--effort`, so the call goes without it.
#[test]
fn claudes_own_effort_variable_never_reaches_the_call() {
    let e = Env::new(
        "effort-env",
        &[
            ("CLAUDE_CODE_EFFORT_LEVEL", Some("max".into())),
            ("COCKPIT_CLAUDE", Some("$D/claude".into())),
            ("XDG_CACHE_HOME", Some("$D/cache".into())),
        ],
    );
    e.exec("claude", "env > \"$D/env\"\necho ok");
    let mut req = Request::spawn(&Call::from_env(), "S", "p", "haiku");
    assert!(until(|| req.poll().is_some()));
    let env = std::fs::read_to_string(e.dir.join("env")).unwrap();
    assert!(!env.lines().any(|l| l.starts_with("CLAUDE_CODE_EFFORT_LEVEL=")), "{env}");
}

// --- #711: a write's prompt hash is of the text it loaded ---

/// An override folder under `e` holding `files`, each `(name, text)`.
fn overrides(e: &Env, files: &[(&str, &str)]) -> PathBuf {
    let d = e.dir.join("cfg/tmux-cockpit/prompts");
    std::fs::create_dir_all(&d).unwrap();
    for (name, text) in files {
        std::fs::write(d.join(format!("{name}.txt")), text).unwrap();
    }
    d
}

fn shipped_examples() -> &'static str {
    prompts::WRITE_EXAMPLES.trim_end_matches('\n')
}

/// The hash covers the files the write read, as read: trimmed, and the explore
/// file only for a write that explores.
#[test]
fn a_write_hashes_the_prompt_files_it_loaded() {
    let e = Env::new("hash-loaded", &[("XDG_CONFIG_HOME", Some("$D/cfg".into()))]);
    overrides(&e, &[("write", "mine\n"), ("write.explore", "look\n")]);
    let ex = shipped_examples();
    let (_, _, plain) = prompts::write_noted(&Ctx::default());
    assert_eq!(plain, prompts::files_hash(&["mine", ex]));
    let (_, _, explore) = prompts::write_explore_noted(&Ctx::default());
    assert_eq!(explore, prompts::files_hash(&["mine", ex, "look"]));
}

/// Each read of the kernel's uuid file differs, as a file edited between two
/// reads would: the hash must come from the read the prompt was built from.
#[test]
fn the_hash_is_of_the_read_the_prompt_came_from() {
    let uuid = Path::new("/proc/sys/kernel/random/uuid");
    if !uuid.exists() {
        eprintln!("SKIP the_hash_is_of_the_read_the_prompt_came_from: no {}", uuid.display());
        return;
    }
    let e = Env::new("hash-one-read", &[("XDG_CONFIG_HOME", Some("$D/cfg".into()))]);
    let d = overrides(&e, &[]);
    std::os::unix::fs::symlink(uuid, d.join("write.txt")).unwrap();
    // the uuid has no {{examples}}, so the system prompt is the text read
    let (system, _, h) = prompts::write_noted(&Ctx::default());
    assert_eq!(h, prompts::files_hash(&[&system, shipped_examples()]));
}

/// As above, for the explore text: an exploring write's hash covers the explore
/// read its prompt ends with, not a second one.
#[test]
fn an_exploring_writes_hash_is_of_the_explore_read_it_sent() {
    let uuid = Path::new("/proc/sys/kernel/random/uuid");
    if !uuid.exists() {
        eprintln!(
            "SKIP an_exploring_writes_hash_is_of_the_explore_read_it_sent: no {}",
            uuid.display()
        );
        return;
    }
    let e = Env::new("hash-explore-read", &[("XDG_CONFIG_HOME", Some("$D/cfg".into()))]);
    let d = overrides(&e, &[]);
    std::os::unix::fs::symlink(uuid, d.join("write.explore.txt")).unwrap();
    let (system, _, h) = prompts::write_explore_noted(&Ctx::default());
    // the explore section follows the write prompt's last blank line
    let (_, explore) = system.rsplit_once("\n\n").unwrap();
    let write = prompts::WRITE.trim_end_matches('\n');
    assert_eq!(h, prompts::files_hash(&[write, shipped_examples(), explore]));
}

#[test]
fn a_plain_write_hash_ignores_the_explore_file() {
    let e = Env::new("hash-plain", &[("XDG_CONFIG_HOME", Some("$D/cfg".into()))]);
    let before = prompts::write_noted(&Ctx::default()).2;
    overrides(&e, &[("write.explore", "edited")]);
    assert_eq!(prompts::write_noted(&Ctx::default()).2, before);
}

/// Was the rig's `the_prompt_hash_follows_each_override_explore_included`.
#[test]
fn each_override_file_moves_an_exploring_writes_hash() {
    let e = Env::new("hash-each", &[("XDG_CONFIG_HOME", Some("$D/cfg".into()))]);
    let mut seen = vec![prompts::write_explore_noted(&Ctx::default()).2];
    for name in ["write", "write.examples", "write.explore"] {
        overrides(&e, &[(name, &format!("{name} edited"))]);
        let h = prompts::write_explore_noted(&Ctx::default()).2;
        assert!(!seen.contains(&h), "{name}.txt did not change the hash");
        seen.push(h);
    }
}

/// #760: a Claude pane is Claude whatever case its command reads, as the agents glob
/// is since #508, in a window not named claude too, and so is a version-shaped process
/// title. A longer name is not Claude.
#[test]
fn a_claude_command_in_any_case_is_captured_deeper() {
    let mut bad = Vec::new();
    let rows = [
        ("CLAUDE", "-S -200"),
        ("Claude", "-S -200"),
        // the native install's process title, as basket::claude_cmd reads it
        ("2.1.318", "-S -200"),
        ("claudex", "-S -80"),
    ];
    for (cmd, want) in rows {
        let e = Env::new("capture-case", &tmux_on_path());
        fake_tmux(&e, &format!("work\\t{cmd}\\t/w\\n"));
        capture("%8").unwrap();
        let args = std::fs::read_to_string(e.dir.join("args")).unwrap();
        if !args.trim().ends_with(want) {
            bad.push(format!("{cmd}: want {want}, got {}", args.trim()));
        }
    }
    assert!(bad.is_empty(), "{} of {} rows wrong:\n{}", bad.len(), rows.len(), bad.join("\n"));
}

// --- #853: the panel's calls start with telemetry, updates and error reports off ---

/// The three variables of #853, unset in the panel's env unless a row sets one.
const QUIET: [&str; 3] = ["DISABLE_TELEMETRY", "DISABLE_AUTOUPDATER", "DISABLE_ERROR_REPORTING"];

/// The env lines of one call made under `set`, with the rest of [`QUIET`] unset,
/// and the panel's own values of [`QUIET`] once the call is done, read before
/// [`Env`]'s drop puts the old ones back.
fn call_env(tag: &str, set: &[(&'static str, &str)]) -> (Vec<String>, Vec<Option<String>>) {
    let mut vars: Vec<(&'static str, Option<String>)> = QUIET
        .iter()
        .map(|k| (*k, set.iter().find(|(s, _)| s == k).map(|(_, v)| v.to_string())))
        .collect();
    vars.extend([
        ("COCKPIT_CLAUDE", Some("$D/claude".into())),
        ("XDG_CACHE_HOME", Some("$D/cache".into())),
    ]);
    let e = Env::new(tag, &vars);
    e.exec("claude", "env > \"$D/env\"\necho ok");
    let mut req = Request::spawn(&Call::from_env(), "S", "p", "haiku");
    assert!(until(|| req.poll().is_some()));
    let own = QUIET.iter().map(|k| std::env::var(k).ok()).collect();
    (
        std::fs::read_to_string(e.dir.join("env"))
            .unwrap()
            .lines()
            .map(String::from)
            .collect(),
        own,
    )
}

#[test]
fn the_call_starts_with_the_three_variables_set_when_absent() {
    let (env, _) = call_env("quiet", &[]);
    for k in QUIET {
        assert!(env.contains(&format!("{k}=1")), "{k}=1 missing:\n{}", env.join("\n"));
    }
}

/// claude reads an empty DISABLE_TELEMETRY or DISABLE_ERROR_REPORTING as unset,
/// the documented way to turn either back on, so an empty value is the user's too.
#[test]
fn a_value_the_user_set_reaches_the_call_unchanged_and_the_rest_are_set() {
    let set = [("DISABLE_TELEMETRY", "0"), ("DISABLE_ERROR_REPORTING", "")];
    let (env, _) = call_env("quiet-user", &set);
    let want = ["DISABLE_TELEMETRY=0", "DISABLE_ERROR_REPORTING=", "DISABLE_AUTOUPDATER=1"];
    for w in want {
        assert!(env.iter().any(|l| l == w), "{w} missing:\n{}", env.join("\n"));
    }
}

/// Only the child gets them: the panel's own process, and so the user's shell
/// and every other process it starts, goes on without them.
#[test]
fn the_panels_own_environment_stays_without_the_three() {
    let (_, own) = call_env("quiet-own", &[]);
    assert_eq!(own, [None, None, None], "the panel's own env after a call, {QUIET:?}");
}

// --- #970: an ask's walk up for a `.git` stops at the user's HOME ---

/// `ask::load` hands `load_in` the HOME of this moment, so a `.git` above HOME
/// (here one planted in the dir HOME sits in) is no project, and a file in a
/// plain folder under HOME has that folder as its repo. The planted `.git` sits
/// between HOME and every real folder above, so the developer's checkout or a
/// `.git` above the temp dir changes nothing. Paths are canonical, and HOME's
/// value goes through `load`'s own canonicalizing, so a linked temp dir holds.
/// A walk past HOME finds the planted `.git`, and the read check then refuses
/// that folder for holding HOME; a home set too low has no project at all.
#[test]
fn an_asks_walk_up_stops_at_home() {
    let e = Env::new("askhome", &[("HOME", Some("$D/home".into()))]);
    let d = e.dir.canonicalize().unwrap();
    std::fs::create_dir_all(d.join(".git")).unwrap();
    std::fs::create_dir_all(d.join("home/notes")).unwrap();
    let f = d.join("home/notes/x.md");
    std::fs::write(&f, "a\nb\n").unwrap();
    let got = cockpit_tui::panel::ask::load(&f, 1, 2).map(|a| a.repo);
    assert_eq!(got, Ok(d.join("home/notes")), "a plain folder under HOME, a .git above it");
}

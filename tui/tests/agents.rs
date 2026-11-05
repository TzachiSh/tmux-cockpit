//! `claude agents --json` off the fetch path and its cache file (#492 fix 5).
//! Gates use files and polls, never a clock: the stub waits on a `go` file.
use cockpit_tui::data::agents::{Background, Entry, read_file, set_background};
use cockpit_tui::data::list_panes;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

static ENV: Mutex<()> = Mutex::new(());

fn write_exec(p: &Path, body: &str) {
    std::fs::write(p, body).unwrap();
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// field 22 of a stat line is `start`
fn stat(start: &str) -> String {
    format!("100 (cl aude) S 1 100 100 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 {start} 0 0\n")
}

struct Rig {
    dir: PathBuf,
    old: Vec<(&'static str, Option<String>)>,
    _lock: MutexGuard<'static, ()>,
}

impl Rig {
    /// One agent pane, pid 100, no @cockpit_session, in a fake proc root at start `5`.
    /// `claude` runs `claude_body` and logs one line per call to `forks`.
    fn new(tag: &str, claude_body: &str) -> Rig {
        Rig::with(tag, claude_body, true)
    }

    /// `board` false is a one-shot caller (`--json`, `--rows`): it waits for the lookup.
    fn with(tag: &str, claude_body: &str, board: bool) -> Rig {
        let lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("cockpit-agents-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let d = dir.display().to_string();
        let row = ["%1", "w:0", "win", "claude", "running", "-", "-", "-", "/w", "100", "-", "-", "@1"].join("\u{1f}");
        std::fs::write(dir.join("rows"), row + "\n").unwrap();
        write_exec(
            &dir.join("bin/tmux"),
            &format!("#!/bin/sh\ncase \"$1\" in list-panes) cat {d}/rows ;; show-option) printf '|claude|' ;; esac\n"),
        );
        write_exec(&dir.join("bin/ps"), "#!/bin/sh\nexit 1\n");
        write_exec(&dir.join("bin/git"), "#!/bin/sh\nexit 1\n");
        // `exec $HOME/bin/sleep`: argv[0] keeps the rig's dir, so Drop can still verify the stub
        let real = ["/usr/bin/sleep", "/bin/sleep"]
            .into_iter()
            .find(|p| std::path::Path::new(p).exists());
        std::os::unix::fs::symlink(real.unwrap_or("/bin/sleep"), dir.join("bin/sleep")).unwrap();
        write_exec(&dir.join("bin/claude"), &format!("#!/bin/sh\necho 1 >> {d}/forks\n{claude_body}\n"));
        let root = dir.join("proc");
        std::fs::create_dir_all(root.join("self/task")).unwrap();
        std::fs::create_dir_all(root.join("100/task/100")).unwrap();
        std::fs::write(root.join("100/task/100/children"), "").unwrap();
        std::fs::write(root.join("100/comm"), "claude\n").unwrap();
        std::fs::write(root.join("100/stat"), stat("5")).unwrap();
        set_background(board);
        let path = std::env::var("PATH").unwrap_or_default();
        let vars = [
            ("PATH", format!("{d}/bin:{path}")),
            ("COCKPIT_TMUX", format!("{d}/bin/tmux")),
            ("COCKPIT_PROC_DIR", format!("{d}/proc")),
            ("HOME", d),
            ("COCKPIT_READONLY", "1".into()),
        ];
        let mut old: Vec<_> = vars.iter().map(|(k, _)| (*k, std::env::var(k).ok())).collect();
        old.push(("XDG_CACHE_HOME", std::env::var("XDG_CACHE_HOME").ok()));
        // SAFETY: the ENV mutex is held until Drop restores these.
        unsafe {
            std::env::remove_var("XDG_CACHE_HOME");
            for (k, v) in &vars {
                std::env::set_var(k, v);
            }
        }
        Rig { dir, old, _lock: lock }
    }

    fn proc_dir(&self, pid: &str, comm: &str, kids: &str) {
        let d = self.dir.join("proc").join(pid);
        std::fs::create_dir_all(d.join("task").join(pid)).unwrap();
        std::fs::write(d.join("task").join(pid).join("children"), kids).unwrap();
        std::fs::write(d.join("comm"), format!("{comm}\n")).unwrap();
    }

    fn cache(&self) -> PathBuf {
        self.dir.join(".cache/tmux-cockpit/agents.json")
    }

    fn forks(&self) -> usize {
        std::fs::read_to_string(self.dir.join("forks")).map_or(0, |s| s.lines().count())
    }

    fn sess(&self) -> String {
        list_panes().unwrap().into_iter().next().unwrap().sess
    }

    /// Poll until the file the thread writes last is there.
    fn wait_cache(&self) -> serde_json::Value {
        for _ in 0..2000 {
            if let Ok(s) = std::fs::read_to_string(self.cache()) {
                return serde_json::from_str(&s).unwrap();
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("no cache file appeared");
    }

    fn seed(&self, body: &str) {
        std::fs::create_dir_all(self.cache().parent().unwrap()).unwrap();
        std::fs::write(self.cache(), body).unwrap();
    }
}

/// Is `pid` still this rig's stub? Its cmdline holds the stub's own path, which
/// lives under the rig's dir, so a reused pid of anything else never matches.
fn stub_of(pid: i32, dir: &std::path::Path) -> bool {
    let Ok(cmd) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    let dir = dir.as_os_str().as_encoded_bytes();
    !dir.is_empty() && cmd.windows(dir.len()).any(|w| w == dir)
}

impl Drop for Rig {
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
        // a red run must not leave a stub looping on a file that never comes
        let _ = std::fs::write(self.dir.join("go"), "");
        // AGENTS.md: never signal a pid nobody verified. The stub may be long gone and
        // its pid reused, so kill it only while its cmdline still names this rig's dir.
        if let Ok(p) = std::fs::read_to_string(self.dir.join("stubpid"))
            && let Ok(p) = p.trim().parse::<i32>()
            && stub_of(p, &self.dir)
        {
            // SAFETY: a plain signal to a pid whose cmdline names this rig's own dir.
            unsafe { libc::kill(p, libc::SIGKILL) };
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const ONE: &str = r#"printf '[{"pid":100,"sessionId":"s-100","cwd":"/launch","kind":"interactive"}]'"#;

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// A stub pid that was reused by an unrelated process is not killed: the rig only
/// signals a pid whose cmdline still names its own dir (AGENTS.md).
#[test]
fn the_rig_never_kills_a_pid_that_is_not_its_stub() {
    let mut other = std::process::Command::new("sleep").arg("30").spawn().unwrap();
    {
        let r = Rig::new("reused", ONE);
        std::fs::write(r.dir.join("stubpid"), other.id().to_string()).unwrap();
    }
    let alive = other.try_wait().unwrap().is_none();
    let _ = other.kill();
    let _ = other.wait();
    assert!(alive, "the rig's drop killed a process that was not its stub");
}

/// The positive path: an exec'd stub (argv[0] under the rig's bin) is reaped by Drop.
#[test]
fn the_rig_kills_its_own_execd_stub() {
    let (pid, mut child) = {
        let r = Rig::new("own", ONE);
        let child = std::process::Command::new(r.dir.join("bin/sleep")).arg("30").spawn().unwrap();
        std::fs::write(r.dir.join("stubpid"), child.id().to_string()).unwrap();
        // between fork and exec the child still shows this test's own cmdline; wait for the exec
        let execd = (0..200).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(5));
            stub_of(child.id() as i32, &r.dir)
        });
        assert!(execd, "the stub never showed the rig's dir in its cmdline");
        (child.id(), child)
    };
    let gone = (0..100).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(10));
        matches!(child.try_wait(), Ok(Some(_)))
    });
    if !gone {
        let _ = child.kill();
        let _ = child.wait();
    }
    assert!(gone, "the rig's drop left its own stub {pid} running");
}

/// The mechanism: the fetch waited for `claude agents`. The stub blocks until
/// the test says go, so a fetch that returns did not wait.
#[test]
fn the_fetch_returns_while_the_lookup_is_still_running() {
    let r = Rig::new(
        "slow",
        &format!("touch {d}/started\nwhile [ ! -f {d}/go ]; do sleep 0.02; done\n{ONE}", d = "$HOME"),
    );
    assert_eq!(r.sess(), "", "the first answer is not in yet");
    assert!(!r.cache().exists(), "the stub is still blocked");
    std::fs::write(r.dir.join("go"), "").unwrap();
    r.wait_cache();
    assert_eq!(r.sess(), "s-100", "the next fetch uses the answer");
}

/// At most one lookup at a time: ten fetches while the first is blocked fork once.
#[test]
fn fetches_during_a_lookup_start_no_second_one() {
    let r = Rig::new("one", &format!("while [ ! -f $HOME/go ]; do sleep 0.02; done\n{ONE}"));
    for _ in 0..10 {
        r.sess();
    }
    std::fs::write(r.dir.join("go"), "").unwrap();
    r.wait_cache();
    // the fetches that waited left their pid on the list; the answer settled it
    assert_eq!(r.sess(), "s-100");
    assert_eq!(r.sess(), "s-100");
    assert_eq!(r.forks(), 1);
}

#[test]
fn the_answer_is_written_in_the_agreed_format_mode_600() {
    let r = Rig::new("fmt", ONE);
    r.sess();
    let v = r.wait_cache();
    assert_eq!(v["v"], 1);
    assert!(v["at"].as_u64().unwrap().abs_diff(now()) < 60);
    let e = &v["by_pid"]["100:5"];
    assert_eq!(
        (e["session"].as_str(), e["cwd"].as_str(), e["kind"].as_str()),
        (Some("s-100"), Some("/launch"), Some("interactive"))
    );
    assert_eq!(v["none"], serde_json::json!([]));
    assert_eq!(std::fs::metadata(r.cache()).unwrap().mode() & 0o777, 0o600);
}

/// Atomic: the file is a rename of a temp file, which leaves no temp behind and
/// a new inode where a plain write would reuse the old one.
#[test]
fn the_cache_is_replaced_by_rename_leaving_no_temp() {
    let r = Rig::new("atomic", ONE);
    r.seed("old");
    let before = std::fs::metadata(r.cache()).unwrap().ino();
    r.sess();
    for _ in 0..2000 {
        if std::fs::metadata(r.cache()).unwrap().ino() != before {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_ne!(std::fs::metadata(r.cache()).unwrap().ino(), before);
    let names: Vec<_> = std::fs::read_dir(r.cache().parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["agents.json"]);
}

/// A pid `claude agents` did not list is cached as a negative: three more
/// fetches ask nothing, and the file says so.
#[test]
fn a_pid_claude_does_not_know_is_not_asked_again() {
    let r = Rig::new("neg", "printf '[]'");
    r.sess();
    let v = r.wait_cache();
    assert_eq!(v["none"], serde_json::json!(["100:5"]));
    for _ in 0..3 {
        assert_eq!(r.sess(), "");
    }
    assert_eq!(r.forks(), 1);
}

/// A second case of the class: a failed spawn (no `claude` that runs) rests too.
#[test]
fn a_claude_that_exits_empty_is_not_asked_again_either() {
    let r = Rig::new("neg2", "exit 1");
    r.sess();
    wait_forks(&r, 1);
    // no file to wait on (a failed call writes none): fetch over a window instead
    for _ in 0..100 {
        r.sess();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(r.forks(), 1);
}

fn seeded(r: &Rig, start: &str) {
    r.seed(&format!(
        r#"{{"v":1,"at":{},"by_pid":{{"100:{start}":{{"session":"s-seed","cwd":"/c","kind":"background"}}}},"none":[]}}"#,
        now()
    ));
}

/// Another board's fresh answer is used with no call of our own.
#[test]
fn a_fresh_file_with_a_matching_start_answers_without_a_call() {
    let r = Rig::new("hit", ONE);
    seeded(&r, "5");
    assert_eq!(r.sess(), "s-seed");
    assert_eq!(r.forks(), 0);
}

/// A reused pid has another start time: the entry is absent and the call runs.
#[test]
fn a_pid_with_a_mismatched_start_counts_as_absent() {
    let r = Rig::new("reuse", ONE);
    seeded(&r, "999");
    assert_eq!(r.sess(), "");
    for _ in 0..2000 {
        if r.forks() == 1 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(r.forks(), 1);
}

#[test]
fn the_reader_guards_age_version_syntax_start_and_proc() {
    let r = Rig::new("guards", ONE);
    let root = r.dir.join("proc");
    let at =
        |t: u64, v: u64| format!(r#"{{"v":{v},"at":{t},"by_pid":{{"100:5":{{"session":"a","cwd":"/c","kind":"k"}}}},"none":["100:5"]}}"#);
    let t = now();
    let fresh = Entry {
        session: "a".into(),
        cwd: "/c".into(),
        kind: "k".into(),
    };
    let p = r.dir.join("f.json");
    let rows: [(&str, String, bool); 8] = [
        ("fresh", at(t, 1), true),
        ("exactly 90 s old", at(t - 90, 1), true),
        ("91 s old", at(t - 91, 1), false),
        ("wrong v", at(t, 2), false),
        ("v 0", at(t, 0), false),
        ("unparseable", "{not json".into(), false),
        ("empty", String::new(), false),
        ("array", "[]".into(), false),
    ];
    for (name, body, ok) in rows {
        std::fs::write(&p, body).unwrap();
        let got = read_file(&p, &root, t);
        assert_eq!(got.is_some(), ok, "{name}");
        if ok {
            let s = got.unwrap();
            assert_eq!(s.by_pid.get("100"), Some(&fresh), "{name}");
            assert!(s.none.contains("100"), "{name}");
        }
    }
    // the start moved: the same file is empty of entries, not absent
    std::fs::write(root.join("100/stat"), stat("6")).unwrap();
    std::fs::write(&p, at(t, 1)).unwrap();
    let s = read_file(&p, &root, t).unwrap();
    assert!(s.by_pid.is_empty() && s.none.is_empty(), "start 6 vs key 5");
    // no proc root (macOS): every entry is absent
    let s = read_file(&p, &r.dir.join("nope"), t).unwrap();
    assert!(s.by_pid.is_empty() && s.none.is_empty());
}

/// A file keyed with a path-like pid never reaches outside the proc root.
#[test]
fn a_key_that_is_not_digits_is_dropped() {
    let r = Rig::new("keys", ONE);
    let p = r.dir.join("f.json");
    // what "../100" would reach if the key were not refused
    std::fs::create_dir_all(r.dir.join("100")).unwrap();
    std::fs::write(r.dir.join("100/stat"), stat("5")).unwrap();
    let body = format!(
        r#"{{"v":1,"at":{},"by_pid":{{"../100:5":{{"session":"x"}},"100:":{{"session":"x"}},"100":{{"session":"x"}}}},"none":["../100:5"]}}"#,
        now()
    );
    std::fs::write(&p, body).unwrap();
    let s = read_file(&p, &r.dir.join("proc"), now()).unwrap();
    assert!(s.by_pid.is_empty() && s.none.is_empty());
}

/// The lookup is a child of the board, and the board sits in a pane: a walk of
/// that pane's tree meets a `claude` and the pane, which runs no agent, turns
/// into a row until the call ends.
fn own_pane_stays_no_agent(tag: &str, grandchild: bool, by_ps: bool) {
    let r = Rig::new(
        tag,
        &format!("echo $$ > $HOME/stubpid\nwhile [ ! -f $HOME/go ]; do sleep 0.02; done\n{ONE}"),
    );
    let row = ["%2", "w:1", "board", "bash", "-", "-", "-", "-", "/w", "200", "-", "-", "@2"].join("\u{1f}");
    let mut rows = std::fs::read_to_string(r.dir.join("rows")).unwrap();
    rows.push_str(&(row + "\n"));
    std::fs::write(r.dir.join("rows"), rows).unwrap();
    let root = r.dir.join("proc");
    r.proc_dir("200", "bash", "");
    r.sess();
    let pid = loop {
        if let Ok(p) = std::fs::read_to_string(r.dir.join("stubpid")) {
            break p.trim().to_string();
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    // the stub is the board's child, and it is named claude
    std::fs::write(root.join("200/task/200/children"), &pid).unwrap();
    // the real `claude` forks helpers; one named claude, under the child, counts as the child
    let (comm, kids) = if grandchild { ("node", "999999") } else { ("claude", "") };
    r.proc_dir(&pid, comm, kids);
    if grandchild {
        r.proc_dir("999999", "claude", "");
    }
    if by_ps {
        // no proc root to walk: the fake `ps` is the tree
        std::fs::remove_dir_all(root.join("self")).unwrap();
        let kid = if grandchild {
            format!("999999 {pid} claude\n")
        } else {
            String::new()
        };
        let ps = format!("100 1 claude\n200 1 bash\n{pid} 200 claude\n{kid}");
        std::fs::write(r.dir.join("ps"), ps).unwrap();
        write_exec(&r.dir.join("bin/ps"), &format!("#!/bin/sh\ncat {}/ps\n", r.dir.display()));
    }
    let ids: Vec<_> = list_panes().unwrap().into_iter().map(|p| p.id).collect();
    assert_eq!(ids, ["%1"], "the board's own pane showed as an agent");
    std::fs::write(r.dir.join("go"), "").unwrap();
    r.wait_cache();
}

#[test]
fn the_lookup_child_does_not_make_the_boards_own_pane_an_agent() {
    own_pane_stays_no_agent("selfrow", false, false);
}

#[test]
fn nor_does_a_claude_under_the_lookup_child() {
    own_pane_stays_no_agent("selfrow2", true, false);
}

#[test]
fn the_ps_walk_skips_the_lookup_child_too() {
    own_pane_stays_no_agent("selfrow3", true, true);
}

fn seeded_at(r: &Rig, age: u64, session: &str) {
    r.seed(&format!(
        r#"{{"v":1,"at":{},"by_pid":{{"100:5":{{"session":"{session}","cwd":"/c","kind":"k"}}}},"none":[]}}"#,
        now() - age
    ));
}

fn wait_forks(r: &Rig, n: usize) {
    for _ in 0..2000 {
        if r.forks() >= n {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Gone within `polls` x 5 ms. The deadline kill is 2.5 s after the spawn, so a
/// short window is a kill the code made itself.
fn wait_gone_within(pid: &str, polls: u32) {
    for _ in 0..polls {
        if !Path::new("/proc").join(pid).exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("{pid} is still running");
}

fn wait_gone(pid: &str) {
    wait_gone_within(pid, 2000);
}

/// The mechanism: a positive was served for ever. One 70 s old is served, then
/// re-asked, and the changed session id shows on the next fetch.
#[test]
fn a_session_that_changed_under_the_same_pid_is_picked_up() {
    let r = Rig::new("stale", ONE);
    seeded_at(&r, 70, "s-old");
    assert_eq!(r.sess(), "s-old", "served while the call runs");
    for _ in 0..2000 {
        if r.wait_cache()["by_pid"]["100:5"]["session"] == "s-100" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(r.sess(), "s-100");
    assert_eq!(r.forks(), 1);
}

/// `at` is the time of an answer: fetches over a fresh file leave it as it is.
#[test]
fn fetches_never_restamp_the_file() {
    let r = Rig::new("nostamp", ONE);
    seeded_at(&r, 30, "s-seed");
    let before = std::fs::read_to_string(r.cache()).unwrap();
    for _ in 0..5 {
        assert_eq!(r.sess(), "s-seed");
    }
    assert_eq!((std::fs::read_to_string(r.cache()).unwrap(), r.forks()), (before, 0));
}

/// A second case: a stale positive and a stub that fails: asked once, then rests.
#[test]
fn a_failed_refresh_of_a_stale_positive_rests_a_minute() {
    let r = Rig::new("stalefail", "exit 1");
    seeded_at(&r, 70, "s-old");
    r.sess();
    wait_forks(&r, 1);
    for _ in 0..5 {
        assert_eq!(r.sess(), "s-old");
    }
    assert_eq!(r.forks(), 1);
}

/// One-shot `--json` and `--rows`: no thread to come back to, so the call is waited for.
#[test]
fn a_one_shot_read_waits_for_the_lookup() {
    let r = Rig::with("oneshot", &format!("sleep 0.2\n{ONE}"), false);
    assert_eq!(r.sess(), "s-100");
    assert!(r.cache().exists());
}

/// Several panes, one call: what the answer did not name is not asked again.
#[test]
fn a_one_shot_read_asks_once_for_pids_the_answer_lacks() {
    let r = Rig::with("oneshot2", "printf '[]'", false);
    assert_eq!(r.sess(), "");
    assert_eq!(r.sess(), "");
    assert_eq!(r.forks(), 1);
}

/// #294 on the one-shot path: a hung `claude` is killed at the deadline.
#[test]
fn a_one_shot_read_kills_a_hung_lookup() {
    let r = Rig::with("hung", "echo $$ > $HOME/stubpid\nexec $HOME/bin/sleep 60", false);
    assert_eq!(r.sess(), "");
    let pid = std::fs::read_to_string(r.dir.join("stubpid")).unwrap();
    wait_gone(pid.trim());
}

/// A board that quits mid-lookup takes the child with it.
#[test]
fn the_board_quitting_kills_a_lookup_in_flight() {
    let r = Rig::new("quit", "echo $$ > $HOME/stubpid\nexec $HOME/bin/sleep 60");
    let guard = Background::start();
    r.sess();
    let pid = loop {
        if let Ok(p) = std::fs::read_to_string(r.dir.join("stubpid"))
            && !p.is_empty()
        {
            break p.trim().to_string();
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    drop(guard);
    wait_gone_within(&pid, 200);
}

/// The file is data from disk: a session that is no id never reaches a path.
#[test]
fn a_hostile_session_in_the_file_or_the_answer_is_dropped() {
    let r = Rig::new("hostile", r#"printf '[{"pid":100,"sessionId":"../../x"}]'"#);
    let p = r.dir.join("f.json");
    let at = now();
    let ent = |s: &str| format!(r#""100:5":{{"session":"{s}"}}"#);
    for bad in ["../../x", "a b", "", &"a".repeat(65), "x/y"] {
        std::fs::write(&p, format!(r#"{{"v":1,"at":{at},"by_pid":{{{}}},"none":[]}}"#, ent(bad))).unwrap();
        assert!(read_file(&p, &r.dir.join("proc"), at).unwrap().by_pid.is_empty(), "{bad:?}");
    }
    std::fs::write(
        &p,
        format!(r#"{{"v":1,"at":{at},"by_pid":{{{}}},"none":[]}}"#, ent(&"a-1".repeat(21))),
    )
    .unwrap();
    assert_eq!(read_file(&p, &r.dir.join("proc"), at).unwrap().by_pid.len(), 1);
    r.sess();
    r.wait_cache();
    assert_eq!(r.sess(), "", "the answer's hostile id");
}

/// A FIFO, a symlink and a huge file at the cache path are not read, and the fetch goes on.
#[test]
fn a_fifo_a_symlink_or_a_huge_file_at_the_path_is_not_read() {
    let r = Rig::new("nodes", ONE);
    let (cache, root) = (r.cache(), r.dir.join("proc"));
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    let good = r.dir.join("good.json");
    let body = format!(r#"{{"v":1,"at":{},"by_pid":{{"100:5":{{"session":"s-seed"}}}},"none":[]}}"#, now());
    std::fs::write(&good, &body).unwrap();
    assert!(read_file(&good, &root, now()).is_some(), "the control reads");
    std::os::unix::fs::symlink(&good, &cache).unwrap();
    assert_eq!(read_file(&cache, &root, now()), None, "symlink");
    std::fs::remove_file(&cache).unwrap();
    std::fs::write(&cache, format!("{body}{}", " ".repeat(2 << 20))).unwrap();
    assert_eq!(read_file(&cache, &root, now()), None, "2 MiB");
    std::fs::remove_file(&cache).unwrap();
    let c = std::ffi::CString::new(cache.to_str().unwrap()).unwrap();
    // SAFETY: mkfifo takes a path and a mode.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    assert_eq!(read_file(&cache, &root, now()), None, "fifo");
    assert_eq!(r.sess(), "", "the fetch over a fifo");
}

/// By pid, not by name: a real `claude` beside the lookup child in the board's
/// pane is still an agent, so the pane stays a row.
#[test]
fn a_real_claude_beside_the_lookup_child_keeps_the_pane_an_agent() {
    let r = Rig::new(
        "beside",
        &format!("echo $$ > $HOME/stubpid\nwhile [ ! -f $HOME/go ]; do sleep 0.02; done\n{ONE}"),
    );
    let row = ["%2", "w:1", "board", "bash", "-", "-", "-", "-", "/w", "200", "-", "-", "@2"].join("\u{1f}");
    let rows = std::fs::read_to_string(r.dir.join("rows")).unwrap() + &row + "\n";
    std::fs::write(r.dir.join("rows"), rows).unwrap();
    r.proc_dir("200", "bash", "");
    r.sess();
    let pid = loop {
        if let Ok(p) = std::fs::read_to_string(r.dir.join("stubpid"))
            && !p.is_empty()
        {
            break p.trim().to_string();
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    std::fs::write(r.dir.join("proc/200/task/200/children"), format!("{pid} 777")).unwrap();
    r.proc_dir(&pid, "claude", "");
    r.proc_dir("777", "claude", "");
    let ids: Vec<_> = list_panes().unwrap().into_iter().map(|p| p.id).collect();
    assert_eq!(ids, ["%1", "%2"], "a real claude in the board's pane was hidden");
    std::fs::write(r.dir.join("go"), "").unwrap();
    r.wait_cache();
}

/// On the `ps` path a snapshot can be walked after the call ended: its child is
/// still skipped for a few seconds.
#[test]
fn the_ps_walk_still_skips_a_lookup_child_that_has_just_ended() {
    let r = Rig::new("ghost", &format!("echo $$ > $HOME/stubpid\n{ONE}"));
    let row = ["%2", "w:1", "board", "bash", "-", "-", "-", "-", "/w", "200", "-", "-", "@2"].join("\u{1f}");
    let rows = std::fs::read_to_string(r.dir.join("rows")).unwrap() + &row + "\n";
    std::fs::write(r.dir.join("rows"), rows).unwrap();
    r.sess();
    r.wait_cache();
    let pid = std::fs::read_to_string(r.dir.join("stubpid")).unwrap();
    std::fs::remove_dir_all(r.dir.join("proc/self")).unwrap();
    std::fs::write(r.dir.join("ps"), format!("100 1 claude\n200 1 bash\n{} 200 claude\n", pid.trim())).unwrap();
    write_exec(&r.dir.join("bin/ps"), &format!("#!/bin/sh\ncat {}/ps\n", r.dir.display()));
    let ids: Vec<_> = list_panes().unwrap().into_iter().map(|p| p.id).collect();
    assert_eq!(ids, ["%1"]);
}

/// `at` is the time of an answer: a stub that fails leaves a 70 s-old file as it was.
#[test]
fn a_failed_call_does_not_move_at() {
    let r = Rig::new("atfail", "exit 1");
    seeded_at(&r, 70, "s-old");
    let before = std::fs::read_to_string(r.cache()).unwrap();
    r.sess();
    wait_forks(&r, 1);
    // a write would come within ms of the stub's exit
    for _ in 0..200 {
        assert_eq!(std::fs::read_to_string(r.cache()).unwrap(), before);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// A second case: an answer that is not JSON.
#[test]
fn a_reply_that_is_no_json_array_does_not_move_at() {
    let r = Rig::new("atjunk", "echo not-json");
    seeded_at(&r, 70, "s-old");
    let before = std::fs::read_to_string(r.cache()).unwrap();
    r.sess();
    wait_forks(&r, 1);
    for _ in 0..200 {
        assert_eq!(std::fs::read_to_string(r.cache()).unwrap(), before);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// A one-shot read asks for a pid the file does not list, however fresh the file.
#[test]
fn a_one_shot_asks_for_a_pane_a_fresh_file_lacks() {
    let r = Rig::with("oneshot3", ONE, false);
    r.seed(&format!(r#"{{"v":1,"at":{},"by_pid":{{}},"none":[]}}"#, now()));
    assert_eq!(r.sess(), "s-100");
    assert_eq!(r.forks(), 1);
}

/// The child half of the SIGHUP test: a board with a lookup in flight, hung up on.
#[test]
fn hup_child() {
    if std::env::var_os("HUP_CHILD").is_none() {
        return;
    }
    let _board = Background::start();
    let _push = cockpit_tui::push::Push::start();
    list_panes();
    let stub = Path::new(&std::env::var("HOME").unwrap()).join("stubpid");
    while std::fs::read_to_string(&stub).unwrap_or_default().is_empty() {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    // SAFETY: raises the signal the board's handler is installed for.
    unsafe { libc::raise(libc::SIGHUP) };
    std::thread::sleep(std::time::Duration::from_secs(5));
    std::process::exit(3);
}

/// SIGHUP (a `kill-pane`) and SIGTERM end the board with `_exit`: the lookup child goes too.
#[test]
fn a_signalled_board_kills_a_lookup_in_flight() {
    let r = Rig::new("sighup", "echo $$ > $HOME/stubpid\nexec $HOME/bin/sleep 60");
    let me = std::env::current_exe().unwrap();
    let st = std::process::Command::new(me)
        .args(["--exact", "hup_child", "--test-threads=1"])
        .env("HUP_CHILD", "1")
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert_eq!(st.code(), Some(0), "the handler's _exit(0), not the child's own exit");
    let pid = std::fs::read_to_string(r.dir.join("stubpid")).unwrap();
    wait_gone_within(pid.trim(), 200);
}

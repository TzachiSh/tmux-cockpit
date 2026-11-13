//! The test run's guard against the developer's `claude` (#973): the rows of
//! `tests/claude-guard/runner`, the runner the repo's `.cargo/config.toml`
//! gives cargo, and of the `claude` it puts first on a test binary's PATH.
//!
//! A row starts the runner by its full path on a stand-in for a test binary, a
//! shell script. The stand-in's whole environment is made here: its PATH is
//! this rig's folder with a `claude` of its own that counts its starts, then
//! the default search. So a stand-in that starts `claude` by name finds the
//! guard's, and with the runner broken the rig's, never the developer's.
//! Nothing here sets a variable of this process.
//!
//! In a file of its own and not in tests/run.sh: the runner is what `cargo
//! test` goes through, so its rows run wherever it does, in the tui job too,
//! which never runs the shell suite.
mod fake;
use fake::Scratch;
use std::ffi::OsStr;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const RUNNER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/claude-guard/runner");
const STUB: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/claude-guard/claude");

/// Where cargo puts a test binary, below the target folder.
const TEST: &str = "debug/deps/t-0123456789abcdef";

/// A stand-in that says what it runs as, one `key=value` a line.
const SAYS: &str = "#!/bin/sh\necho \"pid=$$\"\necho \"claude=$(command -v claude)\"\n\
                    echo \"path=$PATH\"\necho \"dir=${COCKPIT_TEST_CLAUDE_GUARD_DIR-}\"\n";

/// A stand-in that starts `claude agents --json` as the product does, by
/// PATH, takes the refusal as the product does, and exits with its first
/// argument, 0 without one.
const STARTS: &str = "#!/bin/sh\nclaude agents --json 2>/dev/null\nexit \"${1:-0}\"\n";

/// One run of the runner: how it ended, what it wrote, the pid it was started
/// as, and what it left in the temp folder.
struct Ran {
    code: Option<i32>,
    /// the signal it died by, and whether that left a core file
    signal: Option<(i32, bool)>,
    out: String,
    err: String,
    pid: u32,
    left: Vec<String>,
}

impl Ran {
    /// The value of the stand-in's `key=` line.
    fn said(&self, key: &str) -> &str {
        let line = self.out.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix('='));
        line.unwrap_or_else(|| panic!("no {key}= line in {:?}, stderr {:?}", self.out, self.err))
    }

    /// The guard's records on stderr, the runner's first, without their level.
    fn records(&self) -> Vec<&str> {
        self.err.lines().filter_map(|l| l.strip_prefix("claude-guard: error: ")).collect()
    }
}

/// The text of `rec` from `key=` to `end`, or to the end of the record.
fn field<'a>(rec: &'a str, key: &str, end: Option<&str>) -> &'a str {
    let from = rec.find(&format!(" {key}=")).unwrap_or_else(|| panic!("no {key}= in {rec:?}"));
    let rest = &rec[from + key.len() + 2..];
    end.and_then(|e| rest.find(e)).map_or(rest, |to| &rest[..to])
}

struct Rig(Scratch);

impl Rig {
    /// A folder of this row's own, with an empty temp folder and the rig's `claude`.
    fn new(tag: &str) -> Rig {
        let name = format!("cockpit-claudeguard-{tag}-{}", std::process::id());
        let s = Scratch::fresh(std::env::temp_dir().join(name));
        std::fs::create_dir_all(s.join("tmp")).unwrap();
        std::fs::create_dir_all(s.join("own")).unwrap();
        fake::write_exec(&s.join("own/claude"), "#!/bin/sh\necho x >> \"$0.started\"\n");
        Rig(s)
    }

    /// The PATH a run starts with.
    fn path(&self) -> String {
        format!("{}:/bin:/usr/bin", self.0.join("own").display())
    }

    /// A stand-in at `rel` below the rig, the script `body`.
    fn binary(&self, rel: &str, body: &str) -> PathBuf {
        let exe = self.0.join(rel);
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fake::write_exec(&exe, body);
        exe
    }

    /// How often the rig's own `claude` was started: the one after the guard's.
    fn own_starts(&self) -> usize {
        std::fs::read_to_string(self.0.join("own/claude.started")).map_or(0, |s| s.lines().count())
    }

    fn command(&self) -> Command {
        let mut c = Command::new(RUNNER);
        c.env_clear().env("PATH", self.path()).env("TMPDIR", self.0.join("tmp"));
        c.env("RUNNER", RUNNER).stdin(Stdio::null());
        c
    }

    fn run(&self, exe: &Path, args: &[&str]) -> Ran {
        let mut c = self.command();
        self.ran(c.arg(exe).args(args))
    }

    fn ran(&self, c: &mut Command) -> Ran {
        let child = c.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let pid = child.id();
        let o = child.wait_with_output().unwrap();
        let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
        let left = std::fs::read_dir(self.0.join("tmp")).map_or(vec![], |d| {
            d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect()
        });
        Ran {
            code: o.status.code(),
            signal: o.status.signal().map(|s| (s, o.status.core_dumped())),
            out: text(&o.stdout),
            err: text(&o.stderr),
            pid,
            left,
        }
    }
}

/// The folder the runner made for `ran`: under the rig's temp folder, by its name.
fn guard_dir(r: &Rig, ran: &Ran) -> PathBuf {
    let dir = Path::new(ran.said("claude")).parent().unwrap().to_path_buf();
    assert_eq!(dir.parent(), Some(r.0.join("tmp").as_path()), "{dir:?}");
    let name = dir.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("cockpit-claude-guard."), "{name}");
    dir
}

#[test]
fn a_test_binary_finds_the_guards_claude_first_and_its_old_path_after() {
    let r = Rig::new("first");
    let ran = r.run(&r.binary(TEST, SAYS), &[]);
    let dir = guard_dir(&r, &ran);
    assert_eq!(ran.said("path"), format!("{}:{}", dir.display(), r.path()));
    assert_eq!(ran.said("dir"), dir.to_string_lossy());
    assert_ne!(ran.said("pid"), ran.pid.to_string(), "the runner stays to read the log");
    assert_eq!((ran.code, ran.err.as_str(), r.own_starts()), (Some(0), "", 0));
}

/// What cargo starts through the runner, and whether it is a test binary:
/// (row, the path below the target folder, an argument, guarded).
const RULE: [(&str, &str, &str, bool); 11] = [
    ("cargo test", TEST, "--quiet", true),
    ("cargo test --release", "release/deps/t-0123456789abcdef", "--quiet", true),
    ("an example built as a test", "debug/examples/ev-0123456789abcdef", "--quiet", true),
    ("cargo test --doc, cargo 1.89 on", "rustdoctestAb12Cd/rust_out", "", true),
    ("cargo run", "debug/cockpit-tui", "--quiet", false),
    ("cargo run, target below a deps/", "deps/target/debug/cockpit-tui", "", false),
    ("cargo run, a binary named deps", "debug/deps", "--quiet", false),
    ("cargo run --example", "debug/examples/panel_eval", "--quiet", false),
    ("an example with a dash in its name", "debug/examples/panel-eval", "", false),
    ("an example ending in 15 hex", "debug/examples/e-0123456789abcde", "", false),
    ("cargo bench", "release/deps/t-0123456789abcdef", "--bench", false),
];

#[test]
fn only_what_cargo_test_runs_gets_the_guard() {
    for (row, rel, arg, guarded) in RULE {
        let r = Rig::new("rule");
        let ran = r.run(&r.binary(rel, SAYS), &[arg]);
        assert_eq!(ran.code, Some(0), "{row}: {}", ran.err);
        if guarded {
            guard_dir(&r, &ran);
            continue;
        }
        let own = r.0.join("own/claude").display().to_string();
        let got = (ran.said("claude"), ran.said("path"), ran.said("dir"), ran.said("pid"));
        // the same pid: exec'd in place, as cargo would have started it itself
        assert_eq!(got, (own.as_str(), r.path().as_str(), "", ran.pid.to_string().as_str()), "{row}");
    }
}

#[test]
fn a_binary_that_started_claude_fails_and_is_named_with_the_start() {
    let r = Rig::new("reached");
    let exe = r.binary(TEST, STARTS);
    let ran = r.run(&exe, &[]);
    let recs = ran.records();
    assert_eq!((ran.code, recs.len()), (Some(1), 2), "{}", ran.err);
    assert_eq!(field(recs[0], "binary", None), exe.to_string_lossy());
    assert_eq!(field(recs[1], "argv", None), "claude [agents] [--json]");
    assert_eq!(
        field(recs[1], "cwd", Some(" argv=")),
        std::env::current_dir().unwrap().to_string_lossy()
    );
    assert_eq!(r.own_starts(), 0, "the next claude on PATH was started");
}

#[test]
fn a_binary_that_started_no_claude_keeps_its_exit_code() {
    let r = Rig::new("code");
    let exe = r.binary(TEST, "#!/bin/sh\nexit \"$1\"\n");
    for code in [0, 7, 101] {
        let ran = r.run(&exe, &[&code.to_string()]);
        assert_eq!((ran.code, ran.err.as_str()), (Some(code), ""));
    }
}

#[test]
fn a_failing_binary_that_started_claude_keeps_its_code_and_is_still_named() {
    let r = Rig::new("both");
    let exe = r.binary(TEST, STARTS);
    let ran = r.run(&exe, &["101"]);
    assert_eq!(ran.code, Some(101));
    assert_eq!(field(ran.records()[0], "binary", None), exe.to_string_lossy());
}

#[test]
fn the_guards_folder_is_gone_after_a_pass_a_failure_and_a_start() {
    let r = Rig::new("gone");
    let (quiet, starts) = (r.binary("a/deps/q-1", "#!/bin/sh\nexit \"$1\"\n"), r.binary(TEST, STARTS));
    for (exe, arg) in [(&quiet, "0"), (&quiet, "3"), (&starts, "0")] {
        assert_eq!(r.run(exe, &[arg]).left, Vec::<String>::new(), "{exe:?} {arg}");
    }
}

#[test]
fn arguments_and_stdin_reach_the_binary_as_given() {
    use std::io::Write;
    let r = Rig::new("args");
    let exe = r.binary(TEST, "#!/bin/sh\nfor a in \"$@\"; do echo \"arg=[$a]\"; done\ncat\n");
    let mut c = r.command();
    c.arg(&exe).args(["a b", "", "*", "--exact"]).stdin(Stdio::piped());
    let mut child = c.stdout(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(b"typed\n").unwrap();
    let out = child.wait_with_output().unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    assert_eq!(said, "arg=[a b]\narg=[]\narg=[*]\narg=[--exact]\ntyped\n");
}

#[test]
fn a_space_in_the_temp_folder_and_in_the_binarys_path_holds() {
    let r = Rig::new("a space");
    let said = r.run(&r.binary("target dir/debug/deps/t-0123456789abcdef", SAYS), &[]);
    assert!(guard_dir(&r, &said).to_string_lossy().contains("a space"));
    let exe = r.binary("target dir/debug/deps/s-0123456789abcdef", STARTS);
    let ran = r.run(&exe, &[]);
    assert_eq!((ran.code, ran.left.len(), r.own_starts()), (Some(1), 0, 0), "{}", ran.err);
    assert_eq!(field(ran.records()[0], "binary", None), exe.to_string_lossy());
}

#[test]
fn a_call_without_a_binary_is_a_usage_error() {
    let r = Rig::new("usage");
    let ran = r.ran(&mut r.command());
    assert_eq!((ran.code, ran.out.as_str()), (Some(2), ""));
    assert!(ran.err.contains("<binary>"), "{}", ran.err);
}

/// A stand-in that leaves a file beside itself, so a row can hold it never ran.
const MARKS: &str = "#!/bin/sh\necho x > \"$0.ran\"\n";

#[test]
fn without_a_temp_folder_the_binary_is_not_started() {
    let r = Rig::new("notmp");
    let exe = r.binary(TEST, MARKS);
    let mut c = r.command();
    let ran = r.ran(c.env("TMPDIR", r.0.join("absent")).arg(&exe));
    assert_eq!(ran.code, Some(1));
    assert_eq!(field(ran.records()[0], "binary", None), exe.to_string_lossy());
    assert!(!r.0.join(format!("{TEST}.ran")).exists(), "started without the guard");
}

#[test]
fn a_temp_folder_that_would_split_the_path_is_refused_and_left_empty() {
    let r = Rig::new("colon");
    let tmp = r.0.join("tmp/a:b");
    std::fs::create_dir_all(&tmp).unwrap();
    let mut c = r.command();
    let ran = r.ran(c.env("TMPDIR", &tmp).arg(r.binary(TEST, MARKS)));
    assert_eq!((ran.code, ran.records().len()), (Some(1), 1), "{}", ran.err);
    assert!(!r.0.join(format!("{TEST}.ran")).exists(), "started without the guard");
    assert_eq!(std::fs::read_dir(&tmp).unwrap().count(), 0);
}

/// The log is found from the stub's own path, so a child that was handed PATH
/// and nothing else still leaves its line.
#[test]
fn a_start_from_a_child_with_a_cleared_environment_is_logged() {
    let r = Rig::new("cleared");
    let body = "#!/bin/sh\nenv -i \"PATH=$PATH\" sh -c 'claude from-a-child' 2>/dev/null\nexit 0\n";
    let ran = r.run(&r.binary(TEST, body), &[]);
    assert_eq!(ran.code, Some(1), "{}", ran.err);
    assert_eq!(field(ran.records()[1], "argv", None), "claude [from-a-child]");
}

#[test]
fn every_start_of_many_at_once_leaves_a_whole_line() {
    let r = Rig::new("many");
    let body = "#!/bin/sh\nfor n in 1 2 3 4 5 6 7 8 9 10 11 12; do claude \"$n\" 2>/dev/null & done\nwait\n";
    let ran = r.run(&r.binary(TEST, body), &[]);
    let mut got: Vec<u32> = ran.records()[1..].iter().map(|l| field(l, "argv", None)).map(nth).collect();
    got.sort();
    assert_eq!(got, (1..=12).collect::<Vec<u32>>(), "{}", ran.err);
}

/// The number in a record's `claude [n]`.
fn nth(argv: &str) -> u32 {
    let n = argv.strip_prefix("claude [").and_then(|a| a.strip_suffix(']'));
    n.and_then(|n| n.parse().ok()).unwrap_or_else(|| panic!("a torn line: {argv:?}"))
}

#[test]
fn a_binary_that_removed_the_guards_folder_fails_and_is_named() {
    let r = Rig::new("removed");
    let exe = r.binary(TEST, "#!/bin/sh\nrm -rf \"$COCKPIT_TEST_CLAUDE_GUARD_DIR\"\n");
    let ran = r.run(&exe, &[]);
    assert_eq!((ran.code, ran.records().len()), (Some(1), 1), "{}", ran.err);
    assert_eq!(field(ran.records()[0], "binary", Some(" dir=")), exe.to_string_lossy());
}

/// A run inside a run, as a test that starts cargo would make: the inner
/// binary's start is in the inner log alone.
#[test]
fn a_start_inside_an_inner_run_fails_the_inner_binary_alone() {
    let r = Rig::new("inner");
    let inner = r.binary("in/deps/i-0123456789abcdef", STARTS);
    let body = format!("#!/bin/sh\n\"$RUNNER\" '{}' 2>/dev/null\necho \"inner=$?\"\n", inner.display());
    let ran = r.run(&r.binary(TEST, &body), &[]);
    assert_eq!((ran.said("inner"), ran.code, ran.err.as_str()), ("1", Some(0), ""));
}

#[test]
fn a_start_after_a_clean_inner_run_fails_the_outer_binary() {
    let r = Rig::new("outer");
    let inner = r.binary("in/deps/i-0123456789abcdef", "#!/bin/sh\nexit 0\n");
    let body = format!(
        "#!/bin/sh\n\"$RUNNER\" '{}'\necho \"inner=$?\"\nclaude 2>/dev/null\nexit 0\n",
        inner.display()
    );
    let ran = r.run(&r.binary(TEST, &body), &[]);
    assert_eq!((ran.said("inner"), ran.code, ran.records().len()), ("0", Some(1), 2), "{}", ran.err);
}

/// `COCKPIT_TEST_CLAUDE_GUARD=0` is the switch a developer lifts the guard
/// with by hand. Every other value keeps it, and so does none.
#[test]
fn only_the_switch_at_0_lifts_the_guard() {
    let r = Rig::new("switch");
    let exe = r.binary(TEST, SAYS);
    let own = r.0.join("own/claude").display().to_string();
    let mut c = r.command();
    let lifted = r.ran(c.env("COCKPIT_TEST_CLAUDE_GUARD", "0").arg(&exe));
    assert_eq!(
        (lifted.said("claude"), lifted.said("pid")),
        (own.as_str(), lifted.pid.to_string().as_str())
    );
    for on in ["1", "", "00", "off"] {
        let mut c = r.command();
        let kept = r.ran(c.env("COCKPIT_TEST_CLAUDE_GUARD", on).arg(&exe));
        guard_dir(&r, &kept);
    }
}

/// A stand-in that starts `claude` and then dies by the signal named in its
/// first argument, as a test binary that aborts does. It leaves no core file.
const DIES: &str = "#!/bin/sh\nulimit -c 0\nclaude agents --json 2>/dev/null\nkill -s \"$1\" $$\n";

/// cargo names the signal a test binary died by (`signal: 6, SIGABRT`). The
/// runner is that binary's parent, so it dies the same way, after the log is
/// read and the folder removed.
#[test]
fn a_binary_killed_by_a_signal_is_reported_by_that_signal() {
    let r = Rig::new("signal");
    let exe = r.binary(TEST, DIES);
    for (name, n) in [
        ("ABRT", libc::SIGABRT),
        ("SEGV", libc::SIGSEGV),
        ("KILL", libc::SIGKILL),
        ("TERM", libc::SIGTERM),
    ] {
        let ran = r.run(&exe, &[name]);
        assert_eq!(
            (ran.code, ran.signal.map(|s| s.0), ran.left.len()),
            (None, Some(n), 0),
            "{name}: {}",
            ran.err
        );
        assert_eq!(field(ran.records()[0], "binary", None), exe.to_string_lossy(), "{name}");
    }
}

/// The runner dies by ABRT with no core file of its own beside the binary's.
/// It starts here with the core limit as high as this user may set it and in
/// the rig's folder, where a core would land. Where that limit is 0 no
/// process leaves a core, and this holds whatever the runner does.
#[test]
fn the_runner_that_dies_by_the_binarys_signal_leaves_no_core() {
    use std::os::unix::process::CommandExt;
    let r = Rig::new("core");
    let mut c = r.command();
    c.current_dir(&*r.0).arg(r.binary(TEST, DIES)).arg("ABRT");
    // SAFETY: getrlimit and setrlimit are async-signal-safe and allocate nothing.
    unsafe {
        c.pre_exec(|| {
            let mut l = std::mem::zeroed::<libc::rlimit>();
            libc::getrlimit(libc::RLIMIT_CORE, &mut l);
            l.rlim_cur = l.rlim_max;
            libc::setrlimit(libc::RLIMIT_CORE, &l);
            Ok(())
        })
    };
    assert_eq!(r.ran(&mut c).signal, Some((libc::SIGABRT, false)));
}

/// A binary may choose an exit status above 128. One that names a signal which
/// stops or is ignored is passed on as the status it is: sending STOP would
/// leave the runner, and cargo behind it, waiting for good.
#[test]
fn an_exit_status_that_names_no_deadly_signal_is_passed_on_as_it_is() {
    let r = Rig::new("status");
    let exe = r.binary(TEST, "#!/bin/sh\nexit \"$1\"\n");
    for code in [
        128 + libc::SIGSTOP,
        128 + libc::SIGTSTP,
        128 + libc::SIGCHLD,
        128 + libc::SIGCONT,
        200,
        255,
    ] {
        let ran = r.run(&exe, &[&code.to_string()]);
        assert_eq!((ran.code, ran.signal), (Some(code), None), "{}", ran.err);
    }
}

/// A stand-in that says it runs, in a file beside itself, then waits for a
/// line on the fifo beside itself, starts `claude` and ends well.
const WAITS: &str = "#!/bin/sh\necho x > \"$0.up\"\nread -r _ < \"$0.go\"\nclaude late 2>/dev/null\nexit 0\n";

/// TERM sent to the runner alone, the binary still running: the binary runs
/// on, its start is still reported, the folder goes, and the runner then dies
/// by TERM.
#[test]
fn a_signal_to_the_runner_waits_for_the_binary_and_its_log() {
    let r = Rig::new("term");
    let exe = r.binary(TEST, WAITS);
    let (up, go) = (
        r.0.join(format!("{TEST}.up")),
        std::ffi::CString::new(format!("{}.go", exe.display())).unwrap(),
    );
    // SAFETY: `go` is a NUL-terminated path under the rig's folder.
    assert_eq!(unsafe { libc::mkfifo(go.as_ptr(), 0o600) }, 0);
    let mut c = r.command();
    let child = c.arg(&exe).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap();
    // polled, never slept for: gives up after 20 s
    for tries in 0.. {
        if up.exists() {
            break;
        }
        assert!(tries < 2000, "the binary never started");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // SAFETY: the pid of this row's own child, held and not yet waited for.
    assert_eq!(unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) }, 0);
    std::fs::write(go.to_str().unwrap(), "go\n").unwrap();
    let o = child.wait_with_output().unwrap();
    let said = String::from_utf8_lossy(&o.stderr);
    assert_eq!(
        (o.status.signal(), said.contains("argv=claude [late]")),
        (Some(libc::SIGTERM), true),
        "{said}"
    );
    assert_eq!(std::fs::read_dir(r.0.join("tmp")).unwrap().count(), 0);
}

/// What the row below holds of a run: `switch` is COCKPIT_TEST_CLAUDE_GUARD,
/// `dir` the folder the runner says it made, `path` the PATH. Unset or 0, the
/// run is not one the guard was asked for: a binary started by hand, cargo
/// started outside the checkout, or the switch lifted. Else the first `claude`
/// on PATH is the one in that folder.
fn behind_the_guard(switch: Option<&str>, dir: Option<&Path>, path: &OsStr) -> Result<(), String> {
    if matches!(switch, None | Some("0")) {
        return Ok(());
    }
    let first = dir.filter(|d| d.is_absolute()).map(|d| fake::rigs_claude(path, d));
    if matches!(first, Some(Ok(_))) {
        return Ok(());
    }
    Err(format!(
        "COCKPIT_TEST_CLAUDE_GUARD is set and the guard's claude is not first on PATH ({first:?}, folder \
         {dir:?}): another cargo runner took the place of the repo's (.cargo/config.toml, #973). End \
         yours with `exec {RUNNER} \"$@\"`, or set COCKPIT_TEST_CLAUDE_GUARD=0, which runs the tests \
         with no guard, so that one of them may start your own claude (docs/testing.md)."
    ))
}

/// This binary's own run. The repo's config sets the switch for every cargo
/// started inside the checkout, and `make tui-test` sets it too, so a runner
/// of the developer's own that replaced the repo's fails here, by name.
#[test]
fn this_run_is_behind_the_guard_where_one_was_asked_for() {
    let var = |k: &str| std::env::var_os(k);
    let (switch, dir) = (var("COCKPIT_TEST_CLAUDE_GUARD"), var("COCKPIT_TEST_CLAUDE_GUARD_DIR"));
    let switch = switch.as_deref().map(|s| s.to_string_lossy());
    let held = behind_the_guard(switch.as_deref(), dir.as_deref().map(Path::new), &var("PATH").unwrap_or_default());
    if let Err(why) = held {
        panic!("{why}");
    }
}

#[test]
fn a_run_the_guard_was_asked_for_holds_only_with_the_guards_claude_first() {
    let r = Rig::new("asked");
    let (guard, own) = (r.0.join("guard"), r.0.join("own"));
    r.binary("guard/claude", "#!/bin/sh\nexit 126\n");
    let path = |dirs: &[&Path]| std::env::join_paths(dirs).unwrap();
    let (first, second) = (path(&[&guard, &own]), path(&[&own, &guard]));
    assert_eq!(behind_the_guard(Some("1"), Some(&guard), &first), Ok(()));
    for (row, dir, path) in [
        ("another claude first", Some(guard.as_path()), &second),
        ("no folder", None, &first),
    ] {
        let why = behind_the_guard(Some("1"), dir, path).expect_err(row);
        assert!(why.contains("COCKPIT_TEST_CLAUDE_GUARD=0") && why.contains(RUNNER), "{row}: {why}");
    }
}

#[test]
fn a_run_with_the_switch_unset_or_at_0_holds_whatever_is_on_path() {
    let r = Rig::new("lifted");
    let path = std::env::join_paths([r.0.join("own")]).unwrap();
    for switch in [None, Some("0")] {
        assert_eq!(behind_the_guard(switch, None, &path), Ok(()), "{switch:?}");
    }
}

/// The guard's `claude` as the runner places it: a copy in `dir`, with the
/// runner's file beside it when `log` is set. Started by its full path.
fn stub(dir: &Path, log: bool, args: &[&str]) -> std::process::Output {
    fake::write_exec(&dir.join("claude"), &std::fs::read_to_string(STUB).unwrap());
    if log {
        std::fs::write(dir.join("reached"), "").unwrap();
    }
    Command::new(dir.join("claude")).args(args).stdin(Stdio::null()).output().unwrap()
}

#[test]
fn the_stub_refuses_with_126_and_says_the_same_line_to_stderr_and_the_log() {
    let r = Rig::new("stub");
    let out = stub(&r.0, true, &["-p", "a b"]);
    let said = String::from_utf8_lossy(&out.stderr);
    assert_eq!((out.status.code(), out.stdout.len()), (Some(126), 0));
    assert_eq!(std::fs::read_to_string(r.0.join("reached")).unwrap(), said);
    let rec = said.trim_end().strip_prefix("claude-guard: error: ").expect("the record's level");
    assert_eq!(field(rec, "argv", None), "claude [-p] [a b]");
    assert_eq!(field(rec, "ppid", Some(" cwd=")), std::process::id().to_string());
}

#[test]
fn the_stub_started_with_no_argument_logs_the_bare_name() {
    let r = Rig::new("bare");
    let out = stub(&r.0, true, &[]);
    let said = String::from_utf8_lossy(&out.stderr);
    assert_eq!((out.status.code(), field(said.trim_end(), "argv", None)), (Some(126), "claude"));
}

/// Started by hand from the checkout there is no file of the runner's beside
/// it, and it must not leave one in the tree.
#[test]
fn the_stub_writes_no_log_where_the_runner_made_none() {
    let r = Rig::new("nolog");
    let dir = r.0.join("bare");
    std::fs::create_dir_all(&dir).unwrap();
    let out = stub(&dir, false, &["agents"]);
    let left: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!((out.status.code(), left), (Some(126), vec!["claude".into()]));
    assert!(!out.stderr.is_empty(), "the refusal is still said");
}

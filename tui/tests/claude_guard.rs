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

/// What `child` wrote and how it ended, polled for and never waited on for
/// good: one still running after 30 s is killed and the row fails. A runner
/// that sent itself a signal that stops would hold its row, and cargo, for good.
fn ended(mut child: std::process::Child, what: &str) -> std::process::Output {
    for tries in 0.. {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if tries == 6000 {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("{what} had not ended after 30 s and was killed");
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    child.wait_with_output().unwrap()
}

/// The signals the rows send to the runner or have a stand-in send itself.
/// A process starts with its parent's ignored signals and signal mask, and a
/// shell cannot trap a signal ignored when it started: a suite started with
/// `&` from a script ignores INT, one under nohup HUP, and bash keeps a mask
/// that dash clears. KILL can be neither ignored nor blocked.
const SENT: [libc::c_int; 7] = [
    libc::SIGHUP,
    libc::SIGINT,
    libc::SIGQUIT,
    libc::SIGTERM,
    libc::SIGUSR1,
    libc::SIGABRT,
    libc::SIGSEGV,
];

/// Run between fork and exec: each signal of [`SENT`] at its default action
/// and unblocked, so a row measures the runner and not how the suite was
/// started (#1044).
fn at_default() -> std::io::Result<()> {
    // SAFETY: signal, sigemptyset, sigaddset and sigprocmask are async-signal-safe.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for s in SENT {
            libc::signal(s, libc::SIG_DFL);
            libc::sigaddset(&mut set, s);
        }
        libc::sigprocmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
    }
    Ok(())
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
        self.command_of(Path::new(RUNNER))
    }

    /// The rig's environment for `program`, the runner or a copy of it, with
    /// the signals of [`SENT`] at their default.
    fn command_of(&self, program: &Path) -> Command {
        use std::os::unix::process::CommandExt;
        let mut c = Command::new(program);
        c.env_clear().env("PATH", self.path()).env("TMPDIR", self.0.join("tmp"));
        c.env("RUNNER", RUNNER).stdin(Stdio::null());
        // SAFETY: at_default makes only async-signal-safe calls and allocates nothing.
        unsafe { c.pre_exec(at_default) };
        c
    }

    fn run(&self, exe: &Path, args: &[&str]) -> Ran {
        let mut c = self.command();
        self.ran(c.arg(exe).args(args))
    }

    fn ran(&self, c: &mut Command) -> Ran {
        let child = c.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let pid = child.id();
        let o = ended(child, "the runner");
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
/// (row, the path below the target folder, the arguments, guarded).
const RULE: [(&str, &str, &[&str], bool); 15] = [
    ("cargo test", TEST, &["--quiet"], true),
    ("cargo test --release", "release/deps/t-0123456789abcdef", &["--quiet"], true),
    ("an example built as a test", "debug/examples/ev-0123456789abcdef", &[], true),
    ("cargo test --doc, cargo 1.92", "rustdoctestAb12Cd/rust_out", &[], true),
    ("cargo test -- --bench --test", TEST, &["--bench", "--test"], true),
    ("cargo test -- --test --bench", TEST, &["--test", "--bench"], true),
    ("cargo run", "debug/cockpit-tui", &["--quiet"], false),
    ("cargo run, target below a deps/", "deps/target/debug/cockpit-tui", &[], false),
    ("cargo run, a binary named deps", "debug/deps", &["--quiet"], false),
    ("cargo run --example", "debug/examples/panel_eval", &["--quiet"], false),
    ("an example with a dash in its name", "debug/examples/panel-eval", &[], false),
    ("an example ending in 15 hex", "debug/examples/e-0123456789abcde", &[], false),
    (
        "an example ending in 16, one no hex",
        "debug/examples/e-0123456789abcdeg",
        &[],
        false,
    ),
    ("cargo bench", "release/deps/t-0123456789abcdef", &["--bench"], false),
    (
        "cargo bench -- name",
        "release/deps/t-0123456789abcdef",
        &["name", "--bench"],
        false,
    ),
];

#[test]
fn only_what_cargo_test_runs_gets_the_guard() {
    for (row, rel, args, guarded) in RULE {
        let r = Rig::new("rule");
        let ran = r.run(&r.binary(rel, SAYS), args);
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

/// cargo gives the whole path. By hand from the binary's own folder the path
/// is `./<name>`, and the rule reads it from that folder.
#[test]
fn a_path_given_by_hand_is_read_from_the_folder_it_is_given_in() {
    let r = Rig::new("byhand");
    r.binary(TEST, SAYS);
    let name = "t-0123456789abcdef";
    for (cwd, given) in [
        ("debug/deps", format!("./{name}")),
        ("debug", format!("deps/{name}")),
        ("debug/deps", format!("../deps/{name}")),
    ] {
        let mut c = r.command();
        let ran = r.ran(c.current_dir(r.0.join(cwd)).arg(&given));
        assert_eq!(ran.code, Some(0), "{given}: {}", ran.err);
        guard_dir(&r, &ran);
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

/// The runner copies the stub from beside itself. A runner with no stub there
/// cannot guard, and does not start the binary.
#[test]
fn without_the_stub_beside_the_runner_the_binary_is_not_started() {
    let r = Rig::new("nostub");
    let lone = r.binary("lone/runner", &std::fs::read_to_string(RUNNER).unwrap());
    let mut c = r.command_of(&lone);
    let ran = r.ran(c.arg(r.binary(TEST, MARKS)));
    assert_eq!((ran.code, ran.records().len(), ran.left.len()), (Some(1), 1, 0), "{}", ran.err);
    assert!(!r.0.join(format!("{TEST}.ran")).exists(), "started without the guard");
}

/// PATH takes the guard's folder by its whole path, so a test that changes
/// its folder still finds the guard where TMPDIR was given as a relative one.
#[test]
fn a_relative_temp_folder_is_made_whole_before_it_goes_on_path() {
    let r = Rig::new("relative");
    let exe = r.binary(TEST, "#!/bin/sh\ncd / && claude from-the-root 2>/dev/null\nexit 0\n");
    let mut c = r.command();
    let ran = r.ran(c.current_dir(&*r.0).env("TMPDIR", "tmp").arg(&exe));
    assert_eq!((ran.code, r.own_starts(), ran.left.len()), (Some(1), 0, 0), "{}", ran.err);
    assert_eq!(field(ran.records()[1], "argv", None), "claude [from-the-root]");
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
        ("USR1", libc::SIGUSR1),
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

/// Why a box cannot hold the second half of the row below, or None: `pattern`
/// is its `core_pattern`, `hard` this user's hard core limit. Where the pattern
/// pipes cores to a program the kernel ignores a core limit (core(5)), so the
/// runner is dumped whatever it sets. Where the hard limit is 0 nothing is
/// dumped whatever it sets.
fn no_core_half_skipped(pattern: &str, hard: libc::rlim_t) -> Option<String> {
    if pattern.starts_with('|') {
        Some(format!(
            "core_pattern pipes cores to a program, which ignores a core limit: {}",
            pattern.trim()
        ))
    } else {
        (hard == 0).then(|| "this user's hard core limit is 0".to_string())
    }
}

#[test]
fn the_no_core_half_is_skipped_under_a_piped_pattern_or_a_hard_limit_of_0() {
    let apport = "|/usr/share/apport/apport -p%p -s%s -c%c -d%d -P%P -u%u -g%g -- %E\n";
    let piped = no_core_half_skipped(apport, libc::RLIM_INFINITY).expect("a piped pattern");
    assert!(piped.contains("pipes cores") && piped.ends_with("-- %E"), "{piped}");
    assert_eq!(
        no_core_half_skipped("core\n", 0).as_deref(),
        Some("this user's hard core limit is 0")
    );
    let files = [
        ("core\n", 1),
        ("/var/crash/core.%p\n", libc::RLIM_INFINITY),
        ("", 4096),
        ("core|%p\n", 1),
    ];
    for (pattern, hard) in files {
        assert_eq!(no_core_half_skipped(pattern, hard), None, "{pattern:?} {hard}");
    }
}

/// The runner dies by the binary's ABRT, everywhere. Its core limit is 0 by
/// then, so that where a core is a file its own does not land on the
/// binary's: it starts here with the limit as high as this user may set it,
/// in the rig's folder, and must leave no core. That half is skipped where
/// [`no_core_half_skipped`] says why, with a line on the real stderr, past the
/// harness's capture, so a run's log shows that it was.
#[test]
fn the_runner_that_dies_by_the_binarys_signal_leaves_no_core_file() {
    use std::{io::Write, os::unix::process::CommandExt};
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
    let (signal, core) = r.ran(&mut c).signal.expect("the runner did not die by a signal");
    assert_eq!(signal, libc::SIGABRT);
    let pattern = std::fs::read_to_string("/proc/sys/kernel/core_pattern").unwrap_or_default();
    // SAFETY: getrlimit writes one rlimit into the struct it is handed.
    let hard = unsafe {
        let mut l = std::mem::zeroed::<libc::rlimit>();
        libc::getrlimit(libc::RLIMIT_CORE, &mut l);
        l.rlim_max
    };
    match no_core_half_skipped(&pattern, hard) {
        Some(why) => drop(writeln!(std::io::stderr(), "{CORE_ROW}: SKIPPED the no-core half: {why}")),
        None => assert!(!core, "the runner left a core of its own"),
    }
}

/// The row above, as its skipped half names it in a run's log.
const CORE_ROW: &str = "claude_guard the_runner_that_dies_by_the_binarys_signal_leaves_no_core_file";

/// A binary may choose an exit status above 128, and the runner cannot tell it
/// from a death by that signal. The four signals that stop are never sent on:
/// the runner, and cargo behind it, would wait for good. One that ends
/// nothing by default (CHLD, CONT, URG, WINCH) is sent and ends nothing. A
/// status no signal has is left alone. All come out as the status they are.
#[test]
fn an_exit_status_that_names_no_deadly_signal_is_passed_on_as_it_is() {
    let r = Rig::new("status");
    let exe = r.binary(TEST, "#!/bin/sh\nexit \"$1\"\n");
    let stops = [libc::SIGSTOP, libc::SIGTSTP, libc::SIGTTIN, libc::SIGTTOU];
    let ends_nothing = [libc::SIGCHLD, libc::SIGCONT, libc::SIGURG, libc::SIGWINCH];
    for code in stops.into_iter().chain(ends_nothing).map(|n| 128 + n).chain([200, 255]) {
        let ran = r.run(&exe, &[&code.to_string()]);
        assert_eq!((ran.code, ran.signal), (Some(code), None), "{}", ran.err);
    }
}

/// A stand-in that writes its pid to a file beside itself, then waits for a
/// line on the fifo beside itself, starts `claude` and ends well.
const WAITS: &str = "#!/bin/sh\necho $$ > \"$0.up\"\nread -r _ < \"$0.go\"\nclaude late 2>/dev/null\nexit 0\n";

/// `signal` sent to the runner alone while its binary runs: how the runner
/// ended, whether the start the binary made afterwards was reported, and what
/// was left in the temp folder.
fn sent_to_the_runner(signal: i32) -> (Option<i32>, bool, usize) {
    let r = Rig::new(&format!("sent{signal}"));
    let exe = r.binary(TEST, WAITS);
    let (up, go) = (r.0.join(format!("{TEST}.up")), format!("{}.go", exe.display()));
    let fifo = std::ffi::CString::new(go.as_str()).unwrap();
    // SAFETY: `fifo` is a NUL-terminated path under the rig's folder.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let mut c = r.command();
    let child = c.arg(&exe).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap();
    appears(&up);
    // SAFETY: the pid of this row's own child, held and not yet waited for.
    assert_eq!(unsafe { libc::kill(child.id() as libc::pid_t, signal) }, 0);
    if let Err(e) = release(Path::new(&go)) {
        end_stand_in(&up, &exe);
        drop(ended(child, "the runner"));
        panic!("{e}");
    }
    let o = ended(child, "the runner");
    let said = String::from_utf8_lossy(&o.stderr).contains("argv=claude [late]");
    (o.status.signal(), said, std::fs::read_dir(r.0.join("tmp")).unwrap().count())
}

/// A signal sent to the runner alone, the binary still running: the binary
/// runs on, its start is still reported, the folder goes, and the runner then
/// dies by that signal. TERM and HUP come from outside, INT from the keyboard,
/// where the binary gets it too.
#[test]
fn a_signal_to_the_runner_waits_for_the_binary_and_its_log() {
    for signal in [libc::SIGTERM, libc::SIGHUP, libc::SIGINT] {
        assert_eq!(sent_to_the_runner(signal), (Some(signal), true, 0), "signal {signal}");
    }
}

/// The row above, in this binary started again with INT and HUP ignored, as
/// `&` from a script and nohup hand them down. The rig's reset is what holds
/// it (#1044). The inner run must name one passed row: a renamed row runs none.
#[test]
fn the_signal_row_holds_in_a_binary_handed_an_ignored_int_and_hup() {
    use std::os::unix::process::CommandExt;
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", "a_signal_to_the_runner_waits_for_the_binary_and_its_log"]);
    c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // SAFETY: signal is async-signal-safe and allocates nothing.
    unsafe {
        c.pre_exec(|| {
            libc::signal(libc::SIGINT, libc::SIG_IGN);
            libc::signal(libc::SIGHUP, libc::SIG_IGN);
            Ok(())
        })
    };
    let out = ended(c.spawn().unwrap(), "this binary, started again");
    let said = String::from_utf8_lossy(&out.stdout);
    assert_eq!((out.status.code(), said.contains(" 1 passed; 0 failed")), (Some(0), true), "{said}");
}

/// QUIT is the keyboard's other one, ctrl and backslash. The start is still reported and
/// the folder goes. How the runner then ends is its shell's own: dash dies by
/// QUIT, and bash, where it is sh, ignores QUIT in every shell and ends with
/// the binary's status.
#[test]
fn a_quit_to_the_runner_still_reads_the_log_and_removes_the_folder() {
    let (_, reported, left) = sent_to_the_runner(libc::SIGQUIT);
    assert_eq!((reported, left), (true, 0));
}

/// [`WAITS`], and a file beside itself once it has ended: nobody waits for a
/// binary whose runner was killed.
const OUTLIVES: &str = "#!/bin/sh\necho $$ > \"$0.up\"\nread -r _ < \"$0.go\"\nclaude late 2>/dev/null\necho x > \"$0.done\"\n";

/// One line into `fifo` for the binary that waits on it. The fifo is opened
/// without waiting, since a write to a fifo nobody reads would hold the row for
/// good. A binary writes that it runs before it opens the fifo, so the open is
/// tried again while it finds no reader (ENXIO), polled for 20 s as
/// [`appears`] is, and then fails (#997).
fn release(fifo: &Path) -> Result<(), String> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    for tries in 0.. {
        let open = std::fs::OpenOptions::new().write(true).custom_flags(libc::O_NONBLOCK).open(fifo);
        match open {
            Ok(mut f) => return f.write_all(b"go\n").map_err(|e| format!("{}: {e}", fifo.display())),
            Err(e) if e.raw_os_error() == Some(libc::ENXIO) && tries < 2000 => {}
            Err(e) => return Err(format!("no binary waits on {}: {e}", fifo.display())),
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    unreachable!()
}

/// Ends the stand-in at `exe` once its row has failed, so a red run leaves no
/// binary blocked on its fifo. Its pid is the one it wrote to `up`, signalled
/// only where that process's command line still names `exe`.
fn end_stand_in(up: &Path, exe: &Path) {
    let pid = std::fs::read_to_string(up).ok().and_then(|s| s.trim().parse::<libc::pid_t>().ok());
    let Some(pid) = pid else { return };
    let args = match std::fs::read(format!("/proc/{pid}/cmdline")) {
        Ok(a) => String::from_utf8_lossy(&a).replace('\0', " "),
        Err(_) if Path::new("/proc/self").is_dir() => String::new(),
        Err(_) => command_output("ps", &["-o", "args=", "-p", &pid.to_string()]),
    };
    if args.contains(&*exe.to_string_lossy()) {
        // SAFETY: a pid whose command line names this row's own stand-in.
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
}

/// A stand-in that says it runs, then waits for a line on the fifo beside
/// itself and ends well.
const BLOCKS: &str = "#!/bin/sh\n: > \"$0.ran\"\nread -r _ < \"$0.go\"\n";

/// The file `ext` a stand-in at `exe` keeps beside itself.
fn sibling(exe: &Path, ext: &str) -> PathBuf {
    PathBuf::from(format!("{}.{ext}", exe.display()))
}

/// The stand-ins a row started, each ended by [`end_stand_in`] when this is
/// dropped, on every path of the row, a panic's too.
struct Started(Vec<PathBuf>);

impl Drop for Started {
    fn drop(&mut self) {
        self.0.iter().for_each(|exe| end_stand_in(&sibling(exe, "up"), exe));
    }
}

/// The stand-in `name` below the rig, [`BLOCKS`] on a fifo of its own, its pid
/// written beside it as soon as it is started. Returns once its script runs:
/// the command line of a process that has just been started may still be empty
/// for a moment after its exec (#997).
fn blocked(r: &Rig, name: &str, started: &mut Started) -> (PathBuf, std::process::Child) {
    let exe = r.binary(name, BLOCKS);
    let fifo = std::ffi::CString::new(sibling(&exe, "go").to_str().unwrap()).unwrap();
    // SAFETY: `fifo` is a NUL-terminated path under the rig's folder.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let child = Command::new(&exe).stdin(Stdio::null()).spawn().unwrap();
    started.0.push(exe.clone());
    std::fs::write(sibling(&exe, "up"), child.id().to_string()).unwrap();
    appears(&sibling(&exe, "ran"));
    (exe, child)
}

/// [`end_stand_in`] signals the pid in `up` only where its command line names
/// the stand-in it is handed: a pid another process now holds is spared, as
/// AGENTS.md's "Never signal a pid the hook has not verified" asks (#997).
#[test]
fn a_pid_whose_command_line_names_another_binary_is_not_signalled() {
    let r = Rig::new("pid");
    let mut started = Started(vec![]);
    let (spared, a) = blocked(&r, "spared", &mut started);
    let (killed, b) = blocked(&r, "killed", &mut started);
    end_stand_in(&sibling(&spared, "up"), &killed);
    end_stand_in(&sibling(&killed, "up"), &killed);
    let released = release(&sibling(&spared, "go"));
    if released.is_err() {
        end_stand_in(&sibling(&spared, "up"), &spared);
    }
    let (a, b) = (ended(a, "the spared stand-in").status, ended(b, "the killed stand-in").status);
    assert_eq!((released, a.code(), b.signal()), (Ok(()), Some(0), Some(libc::SIGKILL)));
}

/// What `program` wrote to stdout, empty where it could not be run.
fn command_output(program: &str, args: &[&str]) -> String {
    let out = Command::new(program).args(args).stdin(Stdio::null()).output();
    out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

/// [`WAITS`] with a count between its pid file and its fifo, as a loaded box
/// puts time there.
const LATE: &str = "#!/bin/sh\necho $$ > \"$0.up\"\ni=0\nwhile [ $i -lt 100000 ]; do i=$((i + 1)); done\n\
                    read -r l < \"$0.go\"\necho \"$l\"\n";

/// A binary that says it runs and opens its fifo only later still gets its line.
#[test]
fn a_binary_that_opens_its_fifo_late_is_still_released() {
    let r = Rig::new("late");
    let exe = r.binary(TEST, LATE);
    let (up, go) = (r.0.join(format!("{TEST}.up")), r.0.join(format!("{TEST}.go")));
    let fifo = std::ffi::CString::new(go.to_str().unwrap()).unwrap();
    // SAFETY: `fifo` is a NUL-terminated path under the rig's folder.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let child = Command::new(&exe).stdin(Stdio::null()).stdout(Stdio::piped()).spawn().unwrap();
    appears(&up);
    let released = release(&go);
    if released.is_err() {
        end_stand_in(&up, &exe);
    }
    let out = ended(child, "the late binary");
    assert_eq!((released, String::from_utf8_lossy(&out.stdout).as_ref()), (Ok(()), "go\n"));
}

/// Polled, never slept for: gives up after 20 s.
fn appears(file: &Path) {
    for tries in 0.. {
        if file.exists() {
            return;
        }
        assert!(tries < 2000, "{} never appeared", file.display());
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// The runner run by `shell` and killed by USR1, a signal it does not trap,
/// while its binary still waits. The binary then starts `claude`. Returns the
/// folders in the temp folder once the runner was gone, how often the rig's
/// own `claude`, the next on PATH, was started, and the log the guard's folder
/// holds at the end. None where `shell` is at none of `at`, with a line on the
/// real stderr that says which shell.
fn killed_under(shell: &str, at: [&str; 2], flags: &[&str]) -> Option<(usize, usize, String)> {
    use std::io::Write;
    let Some(sh) = at.into_iter().find(|p| Path::new(p).is_file()) else {
        let row = "a_runner_killed_by_a_signal_it_does_not_trap_leaves_the_stub_refusing";
        drop(writeln!(
            std::io::stderr(),
            "claude_guard {row}: SKIPPED under {shell}: no {shell} at {at:?}"
        ));
        return None;
    };
    let r = Rig::new(&format!("killed-{shell}"));
    let exe = r.binary(TEST, OUTLIVES);
    let beside = |ext: &str| PathBuf::from(format!("{}.{ext}", exe.display()));
    let fifo = std::ffi::CString::new(beside("go").to_str().unwrap()).unwrap();
    // SAFETY: `fifo` is a NUL-terminated path under the rig's folder.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let mut c = r.command_of(Path::new(sh));
    let child = c
        .args(flags)
        .arg(RUNNER)
        .arg(&exe)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    appears(&beside("up"));
    // SAFETY: the pid of this row's own child, held and not yet waited for.
    assert_eq!(unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGUSR1) }, 0);
    assert_eq!(ended(child, "the killed runner").status.signal(), Some(libc::SIGUSR1), "{shell}");
    let folders: Vec<PathBuf> = std::fs::read_dir(r.0.join("tmp")).unwrap().map(|e| e.unwrap().path()).collect();
    if let Err(e) = release(&beside("go")) {
        end_stand_in(&beside("up"), &exe);
        panic!("{e}");
    }
    appears(&beside("done"));
    let log = folders
        .first()
        .map(|f| std::fs::read_to_string(f.join("reached")).unwrap_or_default());
    Some((folders.len(), r.own_starts(), log.unwrap_or_default()))
}

/// A runner killed by a signal it does not trap leaves its folder, so the stub
/// keeps refusing for the binary that outlives it. bash runs an EXIT trap on
/// such a death and dash does not: with the trap left in place through the
/// binary, under bash the folder went and the start reached the next `claude`.
/// Each shell runs the runner here itself, whichever of the two `/bin/sh` is.
#[test]
fn a_runner_killed_by_a_signal_it_does_not_trap_leaves_the_stub_refusing() {
    let shells = [
        ("dash", ["/bin/dash", "/usr/bin/dash"], &[][..]),
        ("bash", ["/bin/bash", "/usr/bin/bash"], &["--posix"][..]),
    ];
    for (shell, at, flags) in shells {
        let Some((folders, own, log)) = killed_under(shell, at, flags) else {
            continue;
        };
        assert_eq!((folders, own), (1, 0), "{shell}: the folder is gone or the next claude was started");
        assert!(log.contains("argv=claude [late]"), "{shell}: the stub logged no refusal: {log:?}");
    }
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
    Err(format!("the guard's claude is not first on PATH ({first:?}, folder {dir:?})"))
}

/// The line that names the runner under each target's key in the repo's cargo config.
const RUNNER_LINE: &str = "runner = \"tui/tests/claude-guard/runner\"";

/// What the row below says once it has failed: `found` is what
/// [`behind_the_guard`] found. Choosing the words is a diagnosis and no rule.
/// Where `target`, the one these tests were built for, has no key of its own in
/// `config`, the repo's cargo config, no runner ran, and the words are the two
/// lines to add. Where it has one, or either of the two could not be read,
/// another runner took the repo's place.
fn why_no_guard(found: &str, target: Option<&str>, config: Option<&str>) -> String {
    let lift = "or set COCKPIT_TEST_CLAUDE_GUARD=0, which runs the tests with no guard, so that one of them may \
                start your own claude (docs/testing.md)";
    let keyed = |t: &str, c: &str| c.lines().any(|l| l.trim() == format!("[target.{t}]"));
    match target.zip(config).filter(|(t, c)| !keyed(t, c)) {
        Some((t, _)) => format!(
            "COCKPIT_TEST_CLAUDE_GUARD is set and {found}: the target these tests were built for, {t}, has no \
             runner in the repo's .cargo/config.toml, so no test binary is guarded here (#973). Add \
             there\n[target.{t}]\n{RUNNER_LINE}\n{lift}."
        ),
        None => format!(
            "COCKPIT_TEST_CLAUDE_GUARD is set and {found}: another cargo runner took the place of the repo's \
             (.cargo/config.toml, #973). End yours with `exec {RUNNER} \"$@\"`, {lift}."
        ),
    }
}

/// Where cargo built the binary at `exe`, `<above>/<profile>/deps/<binary>`:
/// the target folder, and the triple when cargo was given `--target`. Read by
/// `.rustc_info.json`, the file cargo keeps its rustc's details in. cargo 1.88
/// and 1.92 write it into the target folder in all four layouts measured (a
/// plain build and a `--target` one, each into a folder cargo made and into one
/// made before) and never into the triple's folder: in `<above>` it is a plain
/// build, in the folder above that a `--target` one (#997). `CACHEDIR.TAG` was
/// read before and cannot tell the four apart. The file is cargo's own and no
/// documented contract. None where neither folder holds it: a binary started
/// by hand, `CARGO_CACHE_RUSTC_INFO=0`, or a cargo that no longer writes it.
/// Then [`built_for`] asks rustc, and [`plain_target`] has none, so the rows
/// that need it print their SKIPPED line. It only chooses words and skips: no
/// row that fails without the guard reads it.
fn built_into(exe: &Path) -> Option<(PathBuf, Option<String>)> {
    let above = exe.parent()?.parent()?.parent()?;
    let made = |d: &Path| d.join(".rustc_info.json").is_file();
    if made(above) {
        return Some((above.to_path_buf(), None));
    }
    let root = above.parent().filter(|r| made(r))?;
    Some((root.to_path_buf(), Some(above.file_name()?.to_string_lossy().into_owned())))
}

/// The target the binary at `exe` was built for: the triple of a `--target`
/// build, and else the one `host` names. None where neither can be read.
fn built_for(exe: &Path, host: impl FnOnce() -> Option<String>) -> Option<String> {
    match built_into(exe) {
        Some((_, Some(triple))) => Some(triple),
        _ => host(),
    }
}

/// The host `rustc -vV` names.
fn rustc_host() -> Option<String> {
    let out = Command::new("rustc").arg("-vV").stdin(Stdio::null()).output().ok()?;
    let said = String::from_utf8_lossy(&out.stdout);
    said.lines().find_map(|l| l.strip_prefix("host: ")).map(str::to_string)
}

/// This binary's own run. The repo's config sets the switch for every cargo
/// started inside the checkout, and `make tui-test` sets it too, so a run with
/// no guard fails here: under a runner of the developer's own that replaced
/// the repo's, and on a target the repo's config has no key for.
#[test]
fn this_run_is_behind_the_guard_where_one_was_asked_for() {
    let var = |k: &str| std::env::var_os(k);
    let (switch, dir) = (var("COCKPIT_TEST_CLAUDE_GUARD"), var("COCKPIT_TEST_CLAUDE_GUARD_DIR"));
    let switch = switch.as_deref().map(|s| s.to_string_lossy());
    let held = behind_the_guard(switch.as_deref(), dir.as_deref().map(Path::new), &var("PATH").unwrap_or_default());
    if let Err(found) = held {
        let config = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../.cargo/config.toml"));
        let target = std::env::current_exe().ok().and_then(|exe| built_for(&exe, rustc_host));
        panic!("{}", why_no_guard(&found, target.as_deref(), config.ok().as_deref()));
    }
}

/// A cargo config with a key for one target, as the repo's has six.
const KEYED: &str = "[env]\nA = \"1\"\n\n[target.x86_64-unknown-linux-gnu]\nrunner = \"r\"\n";

#[test]
fn a_target_with_no_key_in_the_config_is_told_the_two_lines_to_add() {
    let host = "x86_64-unknown-linux-gnu";
    let (commented, longer) = (KEYED.replace("[target.", "# [target."), KEYED.replace("-gnu]", "-gnux32]"));
    let none = [
        ("riscv64gc-unknown-linux-gnu", KEYED),
        (host, commented.as_str()),
        (host, longer.as_str()),
    ];
    for (target, config) in none {
        let why = why_no_guard("found", Some(target), Some(config));
        let lines = format!("\n[target.{target}]\n{RUNNER_LINE}\n");
        assert!(
            why.contains(&lines) && why.contains("COCKPIT_TEST_CLAUDE_GUARD=0"),
            "{target}: {why}"
        );
        assert!(!why.contains("another cargo runner"), "{target}: {why}");
    }
}

/// The general words: for a target that has its key, and wherever the target
/// or the config could not be read.
#[test]
fn a_target_with_a_key_or_one_that_cannot_be_read_is_told_another_runner_took_the_repos_place() {
    let host = "x86_64-unknown-linux-gnu";
    for (row, target, config) in [
        ("keyed", Some(host), Some(KEYED)),
        ("no rustc", None, Some(KEYED)),
        ("no config", Some(host), None),
    ] {
        let why = why_no_guard("found", target, config);
        assert!(why.contains("another cargo runner") && why.contains(RUNNER), "{row}: {why}");
        assert!(
            why.contains("COCKPIT_TEST_CLAUDE_GUARD=0") && !why.contains("[target."),
            "{row}: {why}"
        );
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
        let found = behind_the_guard(Some("1"), dir, path).expect_err(row);
        assert!(found.contains("is not first on PATH"), "{row}: {found}");
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

/// What the child half below writes, with the switch as it found it.
const SWITCH_IS: &str = "the-switch-is=";

/// The child half of the row below: says what the switch is in a run cargo
/// started. Nothing unless that row asked.
#[test]
fn switch_child() {
    if std::env::var_os("CLAUDE_GUARD_SWITCH_CHILD").is_some() {
        eprintln!("{SWITCH_IS}{:?}", std::env::var("COCKPIT_TEST_CLAUDE_GUARD").ok());
    }
}

/// The target folder a plain `cargo test` built the binary at `exe` into,
/// which a second cargo started here finds built: `<target>/debug/deps/<binary>`,
/// by [`built_into`]. None for a binary started by hand or built another way
/// (`--release`, `--target`), where that cargo would build the crate again.
fn plain_target(exe: &Path) -> Option<PathBuf> {
    exe.parent().filter(|d| d.ends_with("debug/deps"))?;
    match built_into(exe)? {
        (target, None) => Some(target),
        (_, Some(_)) => None,
    }
}

/// The triple of a `--target` build in [`LAYOUTS`], its folder's tag and its binary.
const RV: &str = "riscv64gc-unknown-linux-gnu";
const RV_TAG: &str = "riscv64gc-unknown-linux-gnu/CACHEDIR.TAG";
const RV_EXE: &str = "riscv64gc-unknown-linux-gnu/debug/deps/c";
const TAG: &str = "CACHEDIR.TAG";
const INFO: &str = ".rustc_info.json";

/// A target folder as cargo 1.88 and 1.92 left it (#997): (row, the files in it,
/// the binary below it, [`built_for`] with `host` as the host, [`plain_target`]).
type Layout = (
    &'static str,
    &'static [&'static str],
    &'static str,
    &'static str,
    Option<&'static str>,
);

/// `CACHEDIR.TAG` is in the folder cargo made and in a `--target` build's
/// triple folder, and in neither when the folder was made before cargo ran.
const LAYOUTS: [Layout; 7] = [
    ("plain, a folder cargo made", &[TAG, INFO], "debug/deps/c", "host", Some("")),
    ("plain, a folder made before", &[INFO], "debug/deps/c", "host", Some("")),
    ("--target, a folder cargo made", &[TAG, INFO, RV_TAG], RV_EXE, RV, None),
    ("--target, a folder made before", &[INFO, RV_TAG], RV_EXE, RV, None),
    ("plain --release", &[TAG, INFO], "release/deps/c", "host", None),
    ("no .rustc_info.json, plain", &[TAG], "debug/deps/c", "host", None),
    ("no .rustc_info.json, --target", &[TAG, RV_TAG], RV_EXE, "host", None),
];

#[test]
fn the_target_and_the_plain_target_folder_are_read_from_where_cargo_left_rustc_info() {
    for (row, files, exe, target, plain) in LAYOUTS {
        let r = Rig::new("layout");
        let root = r.0.join("t");
        for file in files.iter().chain([&exe]) {
            r.binary(&format!("t/{file}"), "");
        }
        let exe = root.join(exe);
        let got = (built_for(&exe, || Some("host".into())), plain_target(&exe));
        assert_eq!(got, (Some(target.to_string()), plain.map(|p| root.join(p))), "{row}");
    }
}

/// The repo's `.cargo/config.toml` sets the switch to 1 for whatever cargo
/// starts inside the checkout. That is what lets the row above fail under a
/// runner that replaced the repo's, so it is asked of cargo itself: this
/// binary's `switch_child`, started by a cargo of this row's own with the
/// variable taken out of its environment.
#[test]
fn a_cargo_started_in_the_checkout_sets_the_switch_to_1() {
    a_second_cargo_runs_switch_child("a_cargo_started_in_the_checkout_sets_the_switch_to_1", &[]);
}

/// A developer's own runner under a `cfg(..)` key, as one of their configs may
/// hold. `cfg(not(any()))` holds on every target, macOS too, and is a spelling
/// no config holds. cargo merges two tables of one name and this one wins, so
/// a repo key under this exact spelling is the one this row cannot see. Under
/// any other cfg key, `cfg(all())` and `cfg(unix)` among them, the two clash and
/// the row is red (#997). The runner must run the binary it is handed, as `env`
/// does: on a target with no key of the repo's this key is the one that
/// applies, and a runner that failed the binary would turn this row red there
/// too, with no words of its own, and keep it red with the switch at 0 that the
/// guard's row offers (#973).
const OTHER_CFG_RUNNER: &str = "target.'cfg(not(any()))'.runner='env'";

/// The repo's runner is under exact target names (#973). Under a `cfg(..)` key
/// of its own, any other matching cfg runner key a developer holds made cargo
/// refuse every build in the checkout, exit 101, "several matching instances".
/// Under the exact name cargo drops the developer's, and the tests run guarded.
#[test]
fn a_cargo_given_another_matching_cfg_runner_key_still_runs_the_tests() {
    let row = "a_cargo_given_another_matching_cfg_runner_key_still_runs_the_tests";
    a_second_cargo_runs_switch_child(row, &["--config", OTHER_CFG_RUNNER]);
}

/// A cargo of `row`'s own, started in the checkout with `config` added to its
/// arguments, runs this binary's `switch_child`, which must pass with the
/// switch at 1. It finds the crate built in the target folder this binary came
/// from. Skipped, with a line on the real stderr, where no cargo started this
/// binary or it was not built the plain way.
fn a_second_cargo_runs_switch_child(row: &str, config: &[&str]) {
    use std::io::Write;
    let target = std::env::current_exe().ok().and_then(|exe| plain_target(&exe));
    let (Some(cargo), Some(target)) = (std::env::var_os("CARGO"), target) else {
        let why = "no cargo started this binary, or it was not built by a plain cargo test";
        return drop(writeln!(std::io::stderr(), "claude_guard {row}: SKIPPED: {why}"));
    };
    let mut c = Command::new(cargo);
    c.args(["test", "--locked", "--test", "claude_guard", "--target-dir"]).arg(target);
    c.args(config)
        .args(["--", "--exact", "switch_child", "--nocapture"])
        .current_dir(env!("CARGO_MANIFEST_DIR"));
    c.env_remove("COCKPIT_TEST_CLAUDE_GUARD").env("CLAUDE_GUARD_SWITCH_CHILD", "1");
    let child = c
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let out = ended(
        child,
        "the second cargo (a build lock another cargo in the checkout holds, or a rebuild)",
    );
    let said = String::from_utf8_lossy(&out.stderr);
    let switch: Vec<&str> = said.lines().filter_map(|l| l.strip_prefix(SWITCH_IS)).collect();
    assert_eq!(
        (out.status.code(), switch.as_slice()),
        (Some(0), ["Some(\"1\")"].as_slice()),
        "{said}"
    );
}

/// `make tui-test` is the gate, and its recipe sets the switch to 1 on the
/// cargo that runs the tests, whatever the caller exported. The recipe runs
/// here with a `cargo` of the rig's first on PATH, which notes its first
/// argument and the switch it was started with. Skipped, with a line on the
/// real stderr, on a box with no `make`.
#[test]
fn make_tui_test_starts_cargo_test_with_the_switch_at_1_over_an_exported_0() {
    use std::io::Write;
    let r = Rig::new("make");
    let cargo = r.binary(
        "own/cargo",
        "#!/bin/sh\necho \"$1 ${COCKPIT_TEST_CLAUDE_GUARD-unset}\" >> \"$0.calls\"\n",
    );
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut c = Command::new("make");
    c.env_clear().env("PATH", r.path()).env("COCKPIT_TEST_CLAUDE_GUARD", "0");
    c.args(["-s", "-C"]).arg(repo).arg("tui-test").stdin(Stdio::null());
    let child = match c.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
        Ok(child) => child,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let row = "make_tui_test_starts_cargo_test_with_the_switch_at_1_over_an_exported_0";
            return drop(writeln!(std::io::stderr(), "claude_guard {row}: SKIPPED: no make on {}", r.path()));
        }
        Err(e) => panic!("make: {e}"),
    };
    let out = ended(child, "make");
    let calls = std::fs::read_to_string(cargo.with_extension("calls")).unwrap_or_default();
    assert_eq!(
        calls.lines().next(),
        Some("test 1"),
        "{calls}{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The product writes its prompt to `claude`'s stdin and holds the pipe open.
/// The stub reads none of it: here the pipe is never written or closed, and
/// the stub ends all the same.
#[test]
fn the_stub_ends_with_its_stdin_still_open() {
    let r = Rig::new("stdin");
    fake::write_exec(&r.0.join("claude"), &std::fs::read_to_string(STUB).unwrap());
    let mut c = Command::new(r.0.join("claude"));
    let mut child = c.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let held = child.stdin.take();
    assert_eq!(ended(child, "the stub").status.code(), Some(126));
    drop(held);
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

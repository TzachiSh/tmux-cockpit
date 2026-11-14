//! The local store (#1020), each guarantee on a scratch root under TMPDIR, never
//! the developer's state. A row that needs a process-wide setting (umask,
//! RLIMIT_FSIZE) runs in a child of this binary, so it cannot touch the others.
mod fake;

use cockpit_tui::store::{FILE_MAX, Key, LocalStore, PutError, Revision, Store, root_from};
use serde_json::json;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, mpsc};
use std::time::Duration;

/// A hang guard only a red run pays for: a green call returns at once.
const CAP: Duration = Duration::from_secs(30);
const CHILD: &str = "COCKPIT_STORE_TEST_CHILD";

fn scratch(name: &str) -> fake::Scratch {
    fake::fixed_env();
    fake::Scratch::fresh(std::env::temp_dir().join(format!("store-{name}-{}", std::process::id())))
}

fn key(s: &str) -> Key {
    Key::new(s).unwrap()
}

/// The store one level below the scratch dir, so the store makes its own folder.
fn local(dir: &Path) -> LocalStore {
    LocalStore::new(dir.join("store")).unwrap()
}

/// `f` on a thread of its own; `None` when it has not returned within CAP.
fn bounded<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(CAP).ok()
}

/// `path`'s flock, made here so the row does not lean on the put making it. Polled
/// up to CAP: a child another row forks keeps a dropped lock's fd until it execs.
fn hold(path: &Path) -> std::fs::File {
    let f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .unwrap();
    let t0 = std::time::Instant::now();
    // SAFETY: flock on an fd this function owns.
    while unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        assert!(t0.elapsed() < CAP, "no flock on {} within {CAP:?}", path.display());
        std::thread::sleep(Duration::from_millis(5));
    }
    f
}

fn mode(p: &Path) -> u32 {
    std::fs::symlink_metadata(p).unwrap().mode() & 0o777
}

/// This binary again, running only [`child_job`] with `job` on `root`.
fn child(job: &str, root: &Path) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", "child_job", "--test-threads=1", "--quiet"]);
    c.env(CHILD, format!("{job} {}", root.display()));
    c
}

fn told(out: &Output) -> String {
    let (o, e) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    format!("child: {}\nstdout:\n{o}\nstderr:\n{e}", out.status)
}

/// umask 000, and the kernel's SIGXFSZ kill on a write past 4 KiB: a writer
/// stopped dead between its temp file's write and the rename.
fn die_past_4k() {
    let lim = libc::rlimit {
        rlim_cur: 4096,
        rlim_max: 4096,
    };
    // SAFETY: plain syscalls; this child runs one test on one thread.
    unsafe {
        libc::umask(0);
        libc::signal(libc::SIGXFSZ, libc::SIG_DFL);
        assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &lim), 0);
    }
}

/// The other half of [`child`]; a plain run passes it doing nothing.
#[test]
fn child_job() {
    let Ok(job) = std::env::var(CHILD) else { return };
    let (verb, root) = job.split_once(' ').unwrap();
    let (s, k) = (LocalStore::new(root).unwrap(), key("agent.a"));
    match verb {
        "umask0-create" => {
            // SAFETY: as in die_past_4k.
            unsafe { libc::umask(0) };
            std::fs::write(Path::new(root).with_file_name("umask-probe"), "").unwrap();
            s.put_if_revision(&k, &json!({"n": 1}), None).unwrap();
        }
        "killed-mid-write" => {
            die_past_4k();
            let r = s.put_if_revision(&k, &json!({"big": "x".repeat(16 * 1024)}), Some(Revision(1)));
            panic!("lived past the file size limit: {r:?}");
        }
        "race" => {
            let r = s
                .with_lock_wait(CAP)
                .put_if_revision(&k, &json!({"pid": std::process::id()}), Some(Revision(1)));
            std::process::exit(match r {
                Ok(_) => 0,
                Err(PutError::Conflict { .. }) => 3,
                Err(_) => 4,
            });
        }
        _ => panic!("unknown job {verb}"),
    }
}

#[test]
fn a_second_create_conflicts() {
    let d = scratch("create");
    let (s, k) = (local(&d), key("agent.a"));
    assert_eq!(s.put_if_revision(&k, &json!(1), None).unwrap(), Revision(1));
    let again = s.put_if_revision(&k, &json!(2), None);
    assert!(
        matches!(
            again,
            Err(PutError::Conflict {
                current: Some(Revision(1))
            })
        ),
        "{again:?}"
    );
    assert_eq!(s.get(&k), Some((json!(1), Revision(1))));
}

#[test]
fn a_stale_revision_is_refused_with_the_current_one() {
    let d = scratch("stale");
    let (s, k) = (local(&d), key("agent.a"));
    s.put_if_revision(&k, &json!(1), None).unwrap();
    assert_eq!(s.put_if_revision(&k, &json!(2), Some(Revision(1))).unwrap(), Revision(2));
    let stale = s.put_if_revision(&k, &json!(3), Some(Revision(1)));
    assert!(
        matches!(
            stale,
            Err(PutError::Conflict {
                current: Some(Revision(2))
            })
        ),
        "{stale:?}"
    );
    let missing = s.put_if_revision(&key("agent.b"), &json!(1), Some(Revision(1)));
    assert!(matches!(missing, Err(PutError::Conflict { current: None })), "{missing:?}");
    assert_eq!(s.get(&k), Some((json!(2), Revision(2))));
}

/// `n` threads put against `rev` at once; what each got.
fn race_round(s: &LocalStore, k: &Key, rev: Revision, n: usize) -> Vec<Result<Revision, PutError>> {
    let go = Arc::new(Barrier::new(n));
    let hs: Vec<_> = (0..n)
        .map(|w| {
            let (s, k, go) = (s.clone(), k.clone(), go.clone());
            std::thread::spawn(move || {
                go.wait();
                s.put_if_revision(&k, &json!({ "w": w }), Some(rev))
            })
        })
        .collect();
    hs.into_iter().map(|h| h.join().unwrap()).collect()
}

/// A thread reading `k` until told to stop; how many reads found no value.
fn reader(s: &LocalStore, k: &Key, stop: &Arc<AtomicBool>) -> std::thread::JoinHandle<usize> {
    let (s, k, stop) = (s.clone(), k.clone(), stop.clone());
    std::thread::spawn(move || {
        let torn = AtomicUsize::new(0);
        while !stop.load(Ordering::Relaxed) {
            if s.get(&k).is_none() {
                torn.fetch_add(1, Ordering::Relaxed);
            }
        }
        torn.into_inner()
    })
}

#[test]
fn racing_threads_one_winner_per_revision_and_no_torn_read() {
    let d = scratch("race");
    let (s, k) = (local(&d).with_lock_wait(CAP), key("agent.a"));
    let mut rev = s.put_if_revision(&k, &json!({"w": -1}), None).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let read = reader(&s, &k, &stop);
    for round in 0..30 {
        let got = race_round(&s, &k, rev, 8);
        let wins: Vec<Revision> = got.iter().filter_map(|r| r.as_ref().ok().copied()).collect();
        assert_eq!(wins, [Revision(rev.0 + 1)], "round {round}: {got:?}");
        let lost = |r: &&Result<Revision, PutError>| matches!(r, Err(PutError::Conflict { current: Some(c) }) if *c == wins[0]);
        assert_eq!(got.iter().filter(lost).count(), 7, "round {round}: {got:?}");
        rev = wins[0];
    }
    stop.store(true, Ordering::Relaxed);
    assert_eq!(read.join().unwrap(), 0, "reads that found no value");
}

#[test]
fn racing_processes_one_winner() {
    let d = scratch("procs");
    let s = local(&d);
    s.put_if_revision(&key("agent.a"), &json!(0), None).unwrap();
    let spawn = |_| child("race", s.root()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let kids: Vec<_> = (0..6).map(spawn).collect();
    let mut codes: Vec<_> = kids.into_iter().map(|mut c| c.wait().unwrap().code()).collect();
    codes.sort();
    assert_eq!(codes, [Some(0), Some(3), Some(3), Some(3), Some(3), Some(3)]);
    assert_eq!(s.get(&key("agent.a")).map(|(_, r)| r), Some(Revision(2)));
}

#[test]
fn a_writer_killed_before_its_rename_leaves_the_old_value() {
    let d = scratch("kill");
    let (s, k) = (local(&d), key("agent.a"));
    s.put_if_revision(&k, &json!({"old": true}), None).unwrap();
    let out = child("killed-mid-write", s.root()).output().unwrap();
    assert_eq!(out.status.signal(), Some(libc::SIGXFSZ), "{}", told(&out));
    assert_eq!(s.get(&k), Some((json!({"old": true}), Revision(1))));
    assert_eq!(std::fs::metadata(s.temp_path(&k)).unwrap().len(), 4096, "the kill landed mid-write");
    assert_eq!(s.put_if_revision(&k, &json!({"new": 1}), Some(Revision(1))).unwrap(), Revision(2));
    assert_eq!(s.get(&k), Some((json!({"new": 1}), Revision(2))));
}

#[test]
fn files_are_0600_and_the_folder_0700_under_umask_000() {
    let d = scratch("modes");
    let (s, k) = (local(&d), key("agent.a"));
    let made = child("umask0-create", s.root()).output().unwrap();
    assert!(made.status.success(), "{}", told(&made));
    assert_eq!(mode(&d.join("umask-probe")), 0o666, "the child's umask was not 000");
    let killed = child("killed-mid-write", s.root()).output().unwrap();
    assert_eq!(killed.status.signal(), Some(libc::SIGXFSZ), "{}", told(&killed));
    let paths = [s.root().to_path_buf(), s.path(&k), s.lock_path(&k), s.temp_path(&k)];
    assert_eq!(paths.map(|p| mode(&p)), [0o700, 0o600, 0o600, 0o600], "root, file, lock, temp");
}

#[test]
fn a_malformed_or_empty_file_reads_as_none() {
    let d = scratch("malformed");
    let s = local(&d);
    s.put_if_revision(&key("seed"), &json!(0), None).unwrap();
    let big = format!("cockpit-store 1\nrev 1\n\"{}\"\n", "x".repeat(FILE_MAX as usize));
    let deep = format!("cockpit-store 1\nrev 1\n{}{}\n", "[".repeat(200), "]".repeat(200));
    let rows: [(&str, &[u8]); 13] = [
        ("empty", b""),
        ("garbage", b"hello\n"),
        ("version-2", b"cockpit-store 2\nrev 1\n{}\n"),
        ("rev-word", b"cockpit-store 1\nrev x\n{}\n"),
        ("rev-zero", b"cockpit-store 1\nrev 0\n{}\n"),
        ("rev-plus", b"cockpit-store 1\nrev +1\n{}\n"),
        ("rev-overflow", b"cockpit-store 1\nrev 18446744073709551616\n{}\n"),
        ("bad-json", b"cockpit-store 1\nrev 1\n{\n"),
        ("extra-line", b"cockpit-store 1\nrev 1\n{}\n{}\n"),
        ("no-final-newline", b"cockpit-store 1\nrev 1\n{}"),
        ("not-utf8", b"cockpit-store 1\nrev 1\n\"\xff\"\n"),
        ("too-large", big.as_bytes()),
        ("too-deep", deep.as_bytes()),
    ];
    for (name, body) in rows {
        std::fs::write(s.path(&key(name)), body).unwrap();
        assert_eq!(s.get(&key(name)), None, "row {name}");
    }
    std::fs::write(s.path(&key("control")), b"cockpit-store 1\nrev 7\n{\"a\":1}\n").unwrap();
    assert_eq!(s.get(&key("control")), Some((json!({"a": 1}), Revision(7))), "row control");
}

#[test]
fn an_unreadable_file_reads_as_none_and_a_create_starts_fresh() {
    // SAFETY: a plain syscall.
    if unsafe { libc::geteuid() } == 0 {
        return; // root reads a mode 000 file
    }
    let d = scratch("unreadable");
    let (s, k) = (local(&d), key("agent.a"));
    s.put_if_revision(&k, &json!(1), None).unwrap();
    std::fs::set_permissions(s.path(&k), std::fs::Permissions::from_mode(0o000)).unwrap();
    assert_eq!(s.get(&k), None);
    assert_eq!(s.put_if_revision(&k, &json!(2), None).unwrap(), Revision(1));
    assert_eq!(s.get(&k), Some((json!(2), Revision(1))));
}

#[test]
fn a_symlink_or_fifo_at_the_key_is_refused_without_hanging() {
    let d = scratch("special");
    let s = local(&d);
    s.put_if_revision(&key("real"), &json!({"n": 1}), None).unwrap();
    std::os::unix::fs::symlink(s.path(&key("real")), s.path(&key("link"))).unwrap();
    let fifo = std::ffi::CString::new(s.path(&key("fifo")).as_os_str().as_bytes()).unwrap();
    // SAFETY: a plain syscall on a NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let s2 = s.clone();
    assert_eq!(bounded(move || (s2.get(&key("link")), s2.get(&key("fifo")))), Some((None, None)));
}

#[test]
fn a_held_lock_makes_put_busy_not_blocked() {
    let d = scratch("lock");
    let (s, k) = (local(&d).with_lock_wait(Duration::from_millis(50)), key("agent.a"));
    s.put_if_revision(&k, &json!(1), None).unwrap();
    let held = hold(&s.lock_path(&k));
    let (s2, k2) = (s.clone(), k.clone());
    let got = bounded(move || s2.put_if_revision(&k2, &json!(2), Some(Revision(1))));
    assert!(matches!(got, Some(Err(PutError::Busy))), "{got:?}");
    drop(held);
    let after = s.with_lock_wait(CAP).put_if_revision(&k, &json!(2), Some(Revision(1)));
    assert_eq!(after.unwrap(), Revision(2));
}

#[test]
fn a_value_too_large_to_read_back_is_refused() {
    let d = scratch("large");
    let (s, k) = (local(&d), key("agent.a"));
    s.put_if_revision(&k, &json!(1), None).unwrap();
    let got = s.put_if_revision(&k, &json!("x".repeat(FILE_MAX as usize)), Some(Revision(1)));
    assert!(matches!(got, Err(PutError::TooLarge)), "{got:?}");
    assert_eq!(s.get(&k), Some((json!(1), Revision(1))));
}

#[test]
fn a_revision_at_u64_max_is_an_error_not_a_panic() {
    let d = scratch("max");
    let (s, k) = (local(&d), key("agent.a"));
    s.put_if_revision(&key("seed"), &json!(0), None).unwrap();
    std::fs::write(s.path(&k), format!("cockpit-store 1\nrev {}\n{{}}\n", u64::MAX)).unwrap();
    assert_eq!(s.get(&k), Some((json!({}), Revision(u64::MAX))));
    let got = s.put_if_revision(&k, &json!(1), Some(Revision(u64::MAX)));
    assert!(matches!(got, Err(PutError::Io(_))), "{got:?}");
}

#[test]
fn watch_sends_the_keys_under_its_prefix_then_a_later_change() {
    let d = scratch("watch");
    let s = local(&d).with_poll(Duration::from_millis(10));
    for k in ["agent", "agent.a", "agent.b.c", "agentx.a", "other.x"] {
        s.put_if_revision(&key(k), &json!(k), None).unwrap();
    }
    let w = s.watch(&key("agent"));
    let mut first: Vec<_> = std::iter::from_fn(|| w.try_recv())
        .map(|u| (u.key.as_str().to_string(), u.revision))
        .collect();
    first.sort();
    assert_eq!(
        first,
        [("agent.a".to_string(), Revision(1)), ("agent.b.c".to_string(), Revision(1))]
    );
    s.put_if_revision(&key("agent.a"), &json!(2), Some(Revision(1))).unwrap();
    let next = w.recv_timeout(CAP).map(|u| (u.key.as_str().to_string(), u.value, u.revision));
    assert_eq!(next, Some(("agent.a".to_string(), json!(2), Revision(2))));
}

#[test]
fn a_key_is_dotted_tokens_and_nothing_else() {
    let (long, longer) = ("a".repeat(200), "a".repeat(201));
    let ok = ["a", "agent.a-1_B", "msg.agentA.agentB", "-x", &long];
    let bad = [
        "", ".", "..", "a..b", ".a", "a.", "*", "a.*", ">", "a.>", "a/b", "/a", "../a", "a b", "a~lock", "é", "a\n", &longer,
    ];
    for k in ok {
        assert!(Key::new(k).is_ok(), "row {k:?}");
    }
    for k in bad {
        assert_eq!(Key::new(k).map_err(|e| e.key), Err(k.to_string()), "row {k:?}");
    }
}

#[test]
fn the_root_is_absolute_and_under_xdg_state() {
    assert_eq!(
        LocalStore::new("relative/store").map(|_| ()).unwrap_err().kind(),
        std::io::ErrorKind::InvalidInput
    );
    let rows = [
        (Some("/st"), Some("/h"), Some("/st/tmux-cockpit/store")),
        (Some("st"), Some("/h"), Some("/h/.local/state/tmux-cockpit/store")),
        (None, Some("/h"), Some("/h/.local/state/tmux-cockpit/store")),
        (None, Some("h"), None),
        (None, None, None),
    ];
    for (state, home, want) in rows {
        assert_eq!(root_from(state, home), want.map(PathBuf::from), "row {state:?} {home:?}");
    }
}

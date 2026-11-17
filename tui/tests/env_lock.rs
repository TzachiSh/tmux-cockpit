//! The environment lock `fake` owns (#1110): a check reads under it whichever form is
//! called, a guard-less check on the thread that holds it fails every run in place of a
//! wait for itself, as a second take of the lock there does (#1132), a panicked holder leaves the next check its own refusal, and the
//! token a `_held` check takes is made by `fake` alone. A child that `fake::handed` or
//! `fake::run_by_hand` starts takes no write a sibling holds under the lock (#1142).
mod fake;

use std::path::Path;
use std::process::{Command, Stdio};

/// Set in a child of this binary to the case its row's child half runs.
const CHILD: &str = "COCKPIT_TEST_ENV_LOCK_CHILD";

/// A scratch folder named `tag` and this process under cargo's own scratch dir for this
/// target, never the source tree or /tmp, removed when dropped.
fn scratch(tag: &str) -> fake::Scratch {
    let top = Path::new(env!("CARGO_TARGET_TMPDIR"));
    fake::Scratch::fresh(top.join(format!("{tag}-{}", std::process::id())))
}

/// The case this process was started for, where it is a child of a row here.
fn child_case() -> Option<String> {
    std::env::var(CHILD).ok()
}

/// `check` on a thread of its own while this thread holds the lock with `var` set, as a
/// sibling row sets it, then put back: the thread's panic text, or `None` where it passed.
#[cfg(target_os = "linux")]
fn beside_a_write(var: &str, check: fn()) -> Option<String> {
    let held = fake::env_lock();
    let old = std::env::var_os(var);
    // SAFETY: the lock is held, and every write of this binary takes it.
    unsafe { fake::set_var(&held, var, "/sibling-1110") };
    let (tx, rx) = std::sync::mpsc::channel();
    let t = std::thread::spawn(move || {
        tx.send(unsafe { libc::gettid() }).unwrap();
        check();
    });
    fake::asleep_or_done(&t, rx.recv().unwrap());
    // SAFETY: as above, still under the lock.
    unsafe {
        match old {
            Some(v) => fake::set_var(&held, var, v),
            None => fake::remove_var(&held, var),
        }
    }
    drop(held);
    t.join().err().map(|e| e.downcast_ref::<String>().cloned().unwrap_or_default())
}

/// #1110: a guard-less check started while a sibling holds the lock and a variable it reads
/// set waits for the lock, so it reads what the sibling put back, never the sibling's value.
#[test]
#[cfg(target_os = "linux")]
fn a_guard_less_check_reads_once_a_writer_lets_go() {
    let rows: [(&str, &str, fn()); 2] = [
        ("checked_state, a sibling's COCKPIT_LOG", "COCKPIT_LOG", fake::checked_state),
        ("checked_tmux, a sibling's TMUX", "TMUX", || drop(fake::checked_tmux())),
    ];
    let got: Vec<_> = rows.iter().map(|(n, v, check)| (*n, beside_a_write(v, *check))).collect();
    assert_eq!(got, rows.map(|(n, _, _)| (n, None)));
}

/// [`fake::run_by_hand_held`] under the lock, so the child takes no sibling's write of
/// the race row with it.
fn by_hand(test: &str, set: &[(&str, &str)]) -> (Option<i32>, String) {
    fake::run_by_hand_held(&fake::env_lock(), test, set)
}

/// `test` of this binary in a child with `CHILD` at `case`, stdout and stderr in `s`:
/// its exit code and all it wrote, or `None` and what it wrote by then where it is still
/// running after 4000 polls of 5 ms, and then killed.
fn bounded_child(s: &Path, test: &str, case: &str) -> (Option<i32>, String) {
    let out = s.join(format!("{case}.out"));
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", test, "--nocapture"]).env(CHILD, case).stdin(Stdio::null());
    let f = std::fs::File::create(&out).unwrap();
    // under the lock: the child takes no sibling's write of the race row with it
    let mut kid = {
        let _env = fake::env_lock();
        c.stdout(f.try_clone().unwrap()).stderr(f).spawn().unwrap()
    };
    let mut code = None;
    for _ in 0..4000 {
        if let Some(st) = kid.try_wait().unwrap() {
            code = Some(st.code());
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    if code.is_none() {
        let _ = kid.kill();
        let _ = kid.wait();
    }
    (code.flatten(), std::fs::read_to_string(&out).unwrap_or_default())
}

/// #1110: a guard-less check called on the thread that holds the lock panics, naming the
/// `_held` form, where the take would wait for itself, and, reached through a helper, what
/// the caller does instead. A guard-less start of a child does the same, with no helper to
/// name (#1142). Run in a child, so a hang is a kill.
#[test]
fn a_guard_less_check_under_the_lock_names_its_held_form() {
    if let Some(case) = child_case() {
        let _held = fake::env_lock();
        match case.as_str() {
            "state" => fake::checked_state(),
            "helper" => fake::fixed_env(),
            "handed" => drop(fake::handed(&["no-such-row-1142"], &[], &[], CHILD_FOR)),
            "by_hand" => drop(fake::run_by_hand("no-such-row-1142", &[])),
            _ => drop(fake::checked_tmux()),
        }
        return;
    }
    let s = scratch("cockpit-env-lock");
    let me = "a_guard_less_check_under_the_lock_names_its_held_form";
    let named = |case: &str, form: &str| {
        let (code, said) = bounded_child(&s, me, case);
        let held = "on a thread that holds fake::env_lock(): call";
        let want = format!("fake::{form}() {held} fake::{form}_held(&held)");
        (code, said.contains(&want))
    };
    let (code, said) = bounded_child(&s, me, "helper");
    let helper = said.contains("fake::Server::start, call that helper before you take the lock");
    let got = [named("state", "checked_state"), named("tmux", "checked_tmux"), (code, helper)];
    assert_eq!(got, [(Some(101), true); 3]);
    let start = |case: &str, form: &str| {
        let (code, said) = bounded_child(&s, me, case);
        let want = format!(
            "fake::{form}() on a thread that holds fake::env_lock(): call \
                            fake::{form}_held(&held, ..) (#1142)"
        );
        (code, said.contains(&want), said.contains("call that helper"))
    };
    let got = [start("handed", "handed"), start("by_hand", "run_by_hand")];
    assert_eq!(got, [(Some(101), true, false); 2]);
}

/// #1132: a second take of the lock on the thread that holds it panics, naming what the
/// holder does, where the take would wait for itself. Run in a child, so a hang is a kill.
#[test]
fn a_second_take_on_the_holding_thread_names_the_token_to_lend() {
    if child_case().is_some() {
        let _held = fake::env_lock();
        drop(fake::env_lock());
        return;
    }
    let s = scratch("cockpit-env-relock");
    let me = "a_second_take_on_the_holding_thread_names_the_token_to_lend";
    let (code, said) = bounded_child(&s, me, "again");
    let want = "fake::env_lock() on a thread that holds it: lend that token instead";
    assert_eq!((code, said.contains(want)), (Some(101), true), "{said}");
}

/// A row's child: a holder of the lock panics, then the check `case` names runs with its
/// variable kept as a caller's. It must panic with that check's own refusal.
fn check_after_a_panicked_holder(case: &str) {
    let _ = std::panic::catch_unwind(|| {
        let _held = fake::env_lock();
        panic!("a holder's assert failed");
    });
    assert!(fake::env_poisoned(), "the holder's panic left the lock unpoisoned");
    match case {
        "state" => fake::checked_state(),
        "state_held" => fake::checked_state_held(&fake::env_lock()),
        "tmux" => drop(fake::checked_tmux()),
        _ => drop(fake::checked_tmux_held(&fake::env_lock())),
    }
}

/// #1110: a panicked holder poisons the lock, and every form of check after it still gives
/// its own refusal, not a poison panic in its place.
#[test]
fn a_check_after_a_panicked_holder_gives_its_own_refusal() {
    if let Some(case) = child_case() {
        check_after_a_panicked_holder(&case);
        return;
    }
    let me = "a_check_after_a_panicked_holder_gives_its_own_refusal";
    let state = "the caller's state refused (#1066): COCKPIT_LOG set. ";
    let tmux = "tmux refused (#1066): TMUX set. ";
    let rows = [("state", "COCKPIT_LOG", state), ("state_held", "COCKPIT_LOG", state)];
    let rows = rows.into_iter().chain([("tmux", "TMUX", tmux), ("tmux_held", "TMUX", tmux)]);
    let got: Vec<_> = rows
        .map(|(case, var, want)| {
            let (code, said) = by_hand(me, &[(CHILD, case), (var, "/canary-1110")]);
            (case, code, said.contains(want), said.contains("PoisonError"))
        })
        .collect();
    let want = ["state", "state_held", "tmux", "tmux_held"].map(|c| (c, Some(101), true, false));
    assert_eq!(got, want);
}

/// #1110: a guard-less check that refuses lets the lock go first, so its panic poisons
/// nothing. Its child holds that for both checks, with the variable each reads kept.
#[test]
fn a_refusing_guard_less_check_leaves_the_lock_unpoisoned() {
    if let Some(case) = child_case() {
        let refused = std::panic::catch_unwind(|| match case.as_str() {
            "state" => fake::checked_state(),
            _ => drop(fake::checked_tmux()),
        });
        assert_eq!((refused.is_err(), fake::env_poisoned()), (true, false));
        return;
    }
    let me = "a_refusing_guard_less_check_leaves_the_lock_unpoisoned";
    let passed = |case: &str, var: &str| {
        let (code, said) = by_hand(me, &[(CHILD, case), (var, "/canary-1110")]);
        (code, said.contains(&format!("test {me} ... ok")))
    };
    assert_eq!([passed("state", "COCKPIT_LOG"), passed("tmux", "TMUX")], [(Some(0), true); 2]);
}

/// Whether `body`, beside the test-env crate's source loaded as `env` (#1132), builds as a
/// library, and what rustc said. The rustc is the one beside the cargo that built this
/// binary, so the same toolchain; it writes into `s` alone, its TMPDIR too. Where it cannot
/// be started the row fails: there is no path that passes without it.
fn builds_beside_env(s: &Path, name: &str, body: &str) -> (bool, String) {
    let env_rs = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-env/src/lib.rs");
    let src = s.join(format!("{name}.rs"));
    std::fs::write(&src, format!("#[path = {env_rs:?}]\nmod env;\n{body}\n")).unwrap();
    let rustc = Path::new(env!("CARGO")).with_file_name("rustc");
    let out = Command::new(&rustc)
        .args(["--edition", "2024", "--crate-type", "lib", "--emit", "metadata", "--out-dir"])
        .arg(s)
        .arg(&src)
        .env("TMPDIR", s)
        .output()
        .unwrap_or_else(|e| panic!("{}: {e}", rustc.display()));
    (out.status.success(), String::from_utf8_lossy(&out.stderr).into_owned())
}

/// #1110: outside the crate that `fake` re-exports (#1132), the token a `_held` check and a
/// writer take comes from `env_lock` and from nothing else, so a guard of another lock
/// cannot pass for it. Built by rustc: the forged one is refused for its private field
/// (E0451, `_guard`, the parts of the error a toolchain keeps), and the lawful one builds.
#[test]
fn the_token_has_no_constructor_outside_test_env() {
    let s = scratch("cockpit-env-token");
    let taken = "pub fn take() -> env::EnvHeld { env::env_lock() }";
    let forged = "pub fn forge(g: std::sync::MutexGuard<'static, ()>) -> env::EnvHeld {\n\
                  env::EnvHeld { _guard: g }\n}";
    let (ok, said) = builds_beside_env(&s, "taken", taken);
    assert!(ok, "the lawful token did not build: {said}");
    let (ok, said) = builds_beside_env(&s, "forged", forged);
    let refused = said.contains("error[E0451]") && said.contains("`_guard`");
    assert_eq!((ok, refused), (false, true), "{said}");
}

/// Set by the rows below for their child: the file it writes the moment it starts.
const STARTED: &str = "COCKPIT_TEST_ENV_LOCK_STARTED";

/// The variable those rows hold set under the lock, as a sibling holds its write.
const CANARY: &str = "COCKPIT_TEST_ENV_LOCK_CANARY";

/// The child half of the rows below: writes the file [`STARTED`] names, then finds
/// [`CANARY`] unset. Where `STARTED` is unset it is no child and does nothing.
const SEES_NO_CANARY: &str = "a_child_sees_no_canary";

/// How long a row below waits for its child to start under the canary: 1000 polls of
/// 5 ms. With the fix the child never starts while the lock is held, so every green
/// run waits this out once.
const POLLS: usize = 1000;

/// The bound a row below gives a handed child, which passes in well under a second.
const CHILD_FOR: std::time::Duration = std::time::Duration::from_secs(60);

#[test]
fn a_child_sees_no_canary() {
    let Some(started) = std::env::var_os(STARTED) else {
        return;
    };
    std::fs::write(started, "").unwrap();
    assert_eq!(std::env::var_os(CANARY), None, "the child took a write held under the lock");
}

/// `start` on a thread of its own while this thread holds the lock with [`CANARY`] set,
/// as a sibling holds its write: whether the child wrote `started` within [`POLLS`],
/// then, once the canary is removed and the lock let go, what `start` returned. Never a
/// fixed sleep: the poll ends at the child's start or at its bound.
fn beside_a_held_canary(started: &Path, start: impl FnOnce() -> Result<(), String> + Send + 'static) -> (bool, Result<(), String>) {
    let held = fake::env_lock();
    // SAFETY: the lock is held, and every write of this binary takes it.
    unsafe { fake::set_var(&held, CANARY, "/sibling-1142") };
    let t = std::thread::spawn(start);
    let mut began = false;
    for _ in 0..POLLS {
        if started.exists() {
            began = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    // SAFETY: as above, still under the lock.
    unsafe { fake::remove_var(&held, CANARY) };
    drop(held);
    let ran = t
        .join()
        .unwrap_or_else(|e| Err(e.downcast_ref::<String>().cloned().unwrap_or_default()));
    (began, ran)
}

/// Whether a [`fake::run_by_hand`] run of [`SEES_NO_CANARY`] passed: `Err` with all it
/// wrote where it did not.
fn by_hand_passed((code, said): (Option<i32>, String)) -> Result<(), String> {
    let passed = said.contains(&format!("test {SEES_NO_CANARY} ... ok"));
    if code == Some(0) && passed {
        Ok(())
    } else {
        Err(format!("{code:?}: {said}"))
    }
}

/// #1142: a child `fake::handed` starts while a sibling holds the lock with a variable
/// set waits for the lock to start, so it never takes that value. Each green run waits
/// out [`POLLS`] once.
#[test]
fn a_handed_child_starts_once_a_writer_lets_go() {
    let s = scratch("cockpit-env-handed");
    let started = s.join("started");
    let at = started.to_str().unwrap().to_string();
    let got = beside_a_held_canary(&started, move || {
        fake::handed(&[SEES_NO_CANARY], &[], &[(STARTED, &at)], CHILD_FOR).map(drop)
    });
    assert_eq!(got, (false, Ok(())));
}

/// #1142: as above, for a child `fake::run_by_hand` starts. Each green run waits out
/// [`POLLS`] once.
#[test]
fn a_child_run_by_hand_starts_once_a_writer_lets_go() {
    let s = scratch("cockpit-env-by-hand");
    let started = s.join("started");
    let at = started.to_str().unwrap().to_string();
    let got = beside_a_held_canary(&started, move || {
        by_hand_passed(fake::run_by_hand(SEES_NO_CANARY, &[(STARTED, &at)]))
    });
    assert_eq!(got, (false, Ok(())));
}

/// #1142: a caller that holds the lock starts both children through the `_held` forms,
/// where the plain forms would panic on the second take.
#[test]
fn a_holder_starts_children_through_the_held_forms() {
    let s = scratch("cockpit-env-held-forms");
    let started = s.join("started");
    let set = [(STARTED, started.to_str().unwrap())];
    let held = fake::env_lock();
    let handed = fake::handed_held(&held, &[SEES_NO_CANARY], &[], &set, CHILD_FOR).map(drop);
    let by_hand = by_hand_passed(fake::run_by_hand_held(&held, SEES_NO_CANARY, &set));
    drop(held);
    assert_eq!((handed, by_hand), (Ok(()), Ok(())));
}

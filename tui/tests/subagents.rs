//! The subagents reader (#313): `<session>/subagents/agent-<id>.meta.json` and the token
//! figure off `agent-<id>.jsonl`. Its own test binary: the reader keeps a thread-local
//! cache and a read counter, and each test runs on a thread of its own.
use cockpit_tui::data::subagents::{END_UNKNOWN_AFTER, Sub, SubKind, SubState, read_in, reads, state_dir};
use std::fs::{FileTimes, OpenOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

/// A fixture dir, removed when the test ends, pass or fail.
struct Fix(PathBuf);

impl std::ops::Deref for Fix {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Fix {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(name: &str) -> Fix {
    let d = std::env::temp_dir().join(format!("cockpit-subs-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    Fix(d)
}

fn meta(dir: &Path, id: &str, json: &str) {
    std::fs::write(dir.join(format!("agent-{id}.meta.json")), json).unwrap();
}

fn rows(dir: &Path, id: &str, lines: &[&str]) {
    std::fs::write(dir.join(format!("agent-{id}.jsonl")), lines.join("\n") + "\n").unwrap();
}

fn usage(i: u64, o: u64, cc: u64, cr: u64) -> String {
    let u = format!(
        concat!(
            r#"{{"input_tokens":{},"output_tokens":{},"#,
            r#""cache_creation_input_tokens":{},"cache_read_input_tokens":{}}}"#
        ),
        i, o, cc, cr
    );
    let at = r#""timestamp":"2026-11-03T10:00:09Z""#;
    format!(r#"{{"type":"assistant",{at},"message":{{"usage":{u}}}}}"#)
}

const FIRST: &str = concat!(
    r#"{"type":"user","timestamp":"2026-11-03T10:00:00Z","#,
    r#""message":{"role":"user","content":"go"}}"#
);
const T0: u64 = 1_793_700_000; // 2026-11-03T10:00:00Z

fn h(s: &Sub) -> u64 {
    let mut x = DefaultHasher::new();
    s.hash(&mut x);
    x.finish()
}

/// Reads with no hook log: the state root names nothing, so a test never meets the owner's.
fn read(dir: &Path) -> Vec<Sub> {
    read_in(dir, T0 + 60, Path::new("/nonexistent-cockpit-state"))
}

fn one(dir: &Path) -> Sub {
    let v = read(dir);
    assert_eq!(v.len(), 1, "{v:?}");
    v.into_iter().next().unwrap()
}

#[test]
fn no_subagents_dir_reads_nothing() {
    let d = fixture("none");
    let before = reads();
    assert!(read(&d.join("subagents")).is_empty());
    assert_eq!(reads(), before, "no file is opened for a pane without the dir");
}

#[test]
fn a_running_subagent_has_its_type_description_start_and_tokens() {
    let d = fixture("running");
    meta(
        &d,
        "a1",
        r#"{"agentType":"Explore","description":"map the hooks","toolUseId":"toolu_1"}"#,
    );
    rows(&d, "a1", &[FIRST, &usage(10, 20, 30, 40)]);
    let s = one(&d);
    let want = Sub {
        id: "a1".into(),
        kind: SubKind::Subagent,
        ty: "Explore".into(),
        desc: "map the hooks".into(),
        state: SubState::Running,
        started: Some(T0),
        finished_at: None,
        active_at: Some(T0 + 9),
        tokens: 100,
    };
    assert_eq!(s, want);
}

#[test]
fn finished_is_done_an_api_error_row_is_failed_and_a_user_stop_is_stopped() {
    let d = fixture("states");
    meta(&d, "d", r#"{"agentType":"x","finished":true}"#);
    meta(&d, "e", r#"{"agentType":"x","finished":true}"#);
    rows(
        &d,
        "e",
        &[
            FIRST,
            r#"{"type":"assistant","isApiErrorMessage":true,"message":{"role":"assistant"}}"#,
        ],
    );
    meta(&d, "u", r#"{"agentType":"x","stoppedByUser":true}"#);
    meta(&d, "y", r#"{"agentType":"x","finished":"yes"}"#);
    meta(&d, "v", r#"{"agentType":"x","stoppedByUser":"true"}"#);
    let mut got: Vec<(String, SubState)> = read(&d).into_iter().map(|s| (s.id, s.state)).collect();
    got.sort_by(|a, b| a.0.cmp(&b.0));
    let want = [
        ("d", SubState::Done),
        ("e", SubState::Failed),
        ("u", SubState::Stopped),
        ("v", SubState::Running),
        ("y", SubState::Running),
    ];
    let want: Vec<(String, SubState)> = want.iter().map(|(i, s)| (i.to_string(), *s)).collect();
    assert_eq!(got, want, "finished and stoppedByUser must be the boolean true");
}

#[test]
fn a_user_stop_wins_over_an_api_error_row() {
    let d = fixture("stop-wins");
    meta(&d, "s", r#"{"agentType":"x","stoppedByUser":true,"finished":true}"#);
    rows(
        &d,
        "s",
        &[
            FIRST,
            r#"{"type":"assistant","isApiErrorMessage":true,"message":{"role":"assistant"}}"#,
        ],
    );
    assert_eq!(one(&d).state, SubState::Stopped);
}

#[test]
fn stopped_hashes_apart_from_failed_and_done() {
    let s = Sub {
        state: SubState::Stopped,
        ..Default::default()
    };
    for other in [SubState::Failed, SubState::Done, SubState::Running] {
        assert_ne!(h(&s), h(&Sub { state: other, ..s.clone() }), "{other:?}");
    }
}

/// A row of a type that carries no usage, `secs` after `T0` (10:00:00Z).
fn tick(secs: u64) -> String {
    let at = format!("2026-11-03T10:{:02}:{:02}Z", secs / 60, secs % 60);
    format!(r#"{{"type":"attachment","timestamp":"{at}"}}"#)
}

/// Set the meta file's mtime to `T0 + secs`: the write that set its flags.
fn touch_meta(dir: &Path, id: &str, secs: u64) {
    let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(T0 + secs);
    let f = OpenOptions::new()
        .write(true)
        .open(dir.join(format!("agent-{id}.meta.json")))
        .unwrap();
    f.set_times(FileTimes::new().set_modified(t)).unwrap();
}

/// Set the transcript's mtime to `T0 + secs`.
fn touch(dir: &Path, id: &str, secs: u64) {
    let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(T0 + secs);
    let f = OpenOptions::new().write(true).open(dir.join(format!("agent-{id}.jsonl"))).unwrap();
    f.set_times(FileTimes::new().set_modified(t)).unwrap();
}

#[test]
fn finished_at_is_the_newest_rows_timestamp_not_the_files_mtime() {
    let d = fixture("fin-row");
    meta(&d, "a", r#"{"agentType":"x","finished":true}"#);
    rows(&d, "a", &[FIRST, &usage(1, 1, 0, 0), &tick(30)]);
    touch(&d, "a", 5000);
    assert_eq!(one(&d).finished_at, Some(T0 + 30), "a copy or touch moves mtime, not rows");
}

#[test]
fn finished_at_is_the_same_on_a_warm_cache_and_a_fresh_read() {
    let d = fixture("fin-warm");
    meta(&d, "a", r#"{"agentType":"x"}"#);
    rows(&d, "a", &[FIRST, &tick(10)]);
    assert_eq!(one(&d).finished_at, None);
    // a row past the tail window with no timestamp: the tail holds no row with a time
    let big = format!(r#"{{"type":"assistant","message":{{"content":"{}"}}}}"#, "x".repeat(300_000));
    rows(&d, "a", &[FIRST, &tick(10), &big]);
    touch(&d, "a", 900);
    meta(&d, "a", r#"{"agentType":"x","finished":true,"pad":"changes the length"}"#);
    let warm = one(&d).finished_at;
    let path = d.to_path_buf();
    let fresh = std::thread::spawn(move || one(&path).finished_at).join().unwrap();
    assert_eq!((warm, fresh), (Some(T0 + 900), Some(T0 + 900)));
}

#[test]
fn a_stopped_subagent_finishes_at_the_later_of_its_stop_and_its_newest_row() {
    for (name, row, stop, want) in [
        ("stop during a long call: the stop is later", 20, 300, 300),
        ("a closing row after the stop: the row is later", 500, 300, 500),
    ] {
        let d = fixture("fin-stop");
        meta(&d, "s", r#"{"agentType":"x","stoppedByUser":true}"#);
        rows(&d, "s", &[FIRST, &tick(row)]);
        touch_meta(&d, "s", stop);
        let s = one(&d);
        assert_eq!((s.state, s.finished_at), (SubState::Stopped, Some(T0 + want)), "{name}");
    }
}

#[test]
fn a_stopped_subagent_with_no_transcript_finishes_at_its_meta_files_mtime() {
    let d = fixture("fin-stop-bare");
    meta(&d, "s", r#"{"agentType":"x","stoppedByUser":true}"#);
    touch_meta(&d, "s", 300);
    let s = one(&d);
    assert_eq!((s.state, s.finished_at), (SubState::Stopped, Some(T0 + 300)));
}

#[test]
fn the_stopped_path_opens_no_file_beyond_the_meta_and_the_transcript() {
    let d = fixture("fin-stop-reads");
    meta(&d, "s", r#"{"agentType":"x","stoppedByUser":true}"#);
    rows(&d, "s", &[FIRST, &tick(20)]);
    let cold = reads();
    one(&d);
    assert_eq!(reads() - cold, 2, "the meta file and the transcript");
    let warm = reads();
    one(&d);
    assert_eq!(reads(), warm, "a warm read opens nothing");
}

#[test]
fn finished_at_is_none_while_running_and_set_for_every_other_state() {
    let d = fixture("fin-states");
    meta(&d, "r", r#"{"agentType":"x"}"#);
    meta(&d, "d", r#"{"agentType":"x","finished":true}"#);
    meta(&d, "s", r#"{"agentType":"x","stoppedByUser":true}"#);
    meta(&d, "f", r#"{"agentType":"x","finished":true}"#);
    let err = r#"{"type":"assistant","isApiErrorMessage":true,"timestamp":"2026-11-03T10:00:20Z"}"#;
    for id in ["r", "d", "s"] {
        rows(&d, id, &[FIRST, &tick(20)]);
    }
    touch_meta(&d, "s", 5);
    rows(&d, "f", &[FIRST, err]);
    let mut got: Vec<_> = read(&d).into_iter().map(|s| (s.id, s.state, s.finished_at)).collect();
    got.sort_by(|a, b| a.0.cmp(&b.0));
    let at = Some(T0 + 20);
    let want = [
        ("d", SubState::Done, at),
        ("f", SubState::Failed, at),
        ("r", SubState::Running, None),
        ("s", SubState::Stopped, at),
    ];
    let want: Vec<_> = want.iter().map(|(i, st, f)| (i.to_string(), *st, *f)).collect();
    assert_eq!(got, want);
}

#[test]
fn finished_at_falls_back_to_the_mtime_when_no_row_has_a_timestamp() {
    let d = fixture("fin-mtime");
    meta(&d, "a", r#"{"agentType":"x","finished":true}"#);
    rows(&d, "a", &[r#"{"type":"assistant","message":{"role":"assistant"}}"#]);
    touch(&d, "a", 77);
    assert_eq!(one(&d).finished_at, Some(T0 + 77));
}

#[test]
fn finished_at_is_none_for_a_finished_subagent_with_no_transcript() {
    let d = fixture("fin-nofile");
    meta(&d, "a", r#"{"agentType":"x","finished":true}"#);
    let s = one(&d);
    assert_eq!((s.state, s.finished_at), (SubState::Done, None));
}

#[test]
fn finished_at_follows_the_meta_flipping_without_the_transcript_changing() {
    let d = fixture("fin-flip");
    meta(&d, "a", r#"{"agentType":"x"}"#);
    rows(&d, "a", &[FIRST, &tick(40)]);
    assert_eq!(one(&d).finished_at, None);
    meta(&d, "a", r#"{"agentType":"x","finished":true,"pad":"changes the length"}"#);
    assert_eq!(one(&d).finished_at, Some(T0 + 40));
}

#[test]
fn the_hash_ignores_finished_at() {
    let s = Sub {
        ty: "x".into(),
        state: SubState::Done,
        ..Default::default()
    };
    assert_eq!(
        h(&s),
        h(&Sub {
            finished_at: Some(9),
            ..s.clone()
        })
    );
}

// ---------------------------------------------------------------- the hook log (#361)

/// A session on disk: `<root>/<sid>/subagents` for the agents and `<root>/state` for the log
/// dir the hooks write (`tmux-cockpit/subs/<sid>.log`).
struct Sess {
    _root: Fix,
    dir: PathBuf,
    subs: PathBuf,
    log: PathBuf,
}

const SID: &str = "sess1";

fn session(name: &str) -> Sess {
    let root = fixture(name);
    let dir = root.join(SID).join("subagents");
    std::fs::create_dir_all(&dir).unwrap();
    let subs = root.join("state").join("tmux-cockpit").join("subs");
    std::fs::create_dir_all(&subs).unwrap();
    let log = subs.join(format!("{SID}.log"));
    Sess {
        _root: root,
        dir,
        subs,
        log,
    }
}

impl Sess {
    fn log(&self, lines: &[String]) {
        std::fs::write(&self.log, lines.join("\n") + "\n").unwrap();
    }
    fn read(&self, now: u64) -> Vec<Sub> {
        read_in(&self.dir, now, &self.subs)
    }
    fn one(&self, now: u64) -> Sub {
        let v = self.read(now);
        assert_eq!(v.len(), 1, "{v:?}");
        v.into_iter().next().unwrap()
    }
}

/// One hook line: the epoch is `T0 + secs`.
fn ev(secs: u64, event: &str, id: &str) -> String {
    format!("{}\t{event}\t{id}", T0 + secs)
}

const PLAIN: &str = r#"{"agentType":"x"}"#;
const TEAMMATE: &str = r#"{"agentType":"x","taskKind":"in_process_teammate","teamName":"crew"}"#;

#[test]
fn the_last_hook_event_decides_the_state_by_kind() {
    let table: [(&str, &str, &[&str], SubState); 6] = [
        ("start", PLAIN, &["start"], SubState::Running),
        ("start then stop, a subagent", PLAIN, &["start", "stop"], SubState::Done),
        ("start then stop, a teammate", TEAMMATE, &["start", "stop"], SubState::Idle),
        ("a teammate idles", TEAMMATE, &["start", "stop", "idle"], SubState::Idle),
        (
            "a teammate's next message",
            TEAMMATE,
            &["start", "stop", "idle", "start"],
            SubState::Running,
        ),
        ("a resumed subagent", PLAIN, &["start", "stop", "start"], SubState::Running),
    ];
    for (name, m, events, want) in table {
        let s = session("log-last");
        meta(&s.dir, "a", m);
        rows(&s.dir, "a", &[FIRST, &tick(5)]);
        let lines: Vec<String> = events.iter().enumerate().map(|(i, e)| ev(i as u64, e, "a")).collect();
        s.log(&lines);
        assert_eq!(s.one(T0 + 60).state, want, "{name}");
    }
}

#[test]
fn a_user_stop_in_the_meta_wins_over_the_log() {
    let s = session("log-stopped");
    meta(&s.dir, "a", r#"{"agentType":"x","stoppedByUser":true}"#);
    rows(&s.dir, "a", &[FIRST, &tick(5)]);
    s.log(&[ev(0, "start", "a"), ev(5, "stop", "a")]);
    assert_eq!(s.one(T0 + 60).state, SubState::Stopped);
}

#[test]
fn a_stop_after_an_api_error_row_is_failed_for_a_subagent_and_idle_for_a_teammate() {
    let err = r#"{"type":"assistant","isApiErrorMessage":true,"timestamp":"2026-11-03T10:00:20Z"}"#;
    for (m, want) in [(PLAIN, SubState::Failed), (TEAMMATE, SubState::Idle)] {
        let s = session("log-err");
        meta(&s.dir, "a", m);
        rows(&s.dir, "a", &[FIRST, err]);
        s.log(&[ev(0, "start", "a"), ev(20, "stop", "a")]);
        assert_eq!(s.one(T0 + 60).state, want);
    }
}

#[test]
fn a_start_with_no_transcript_write_for_15_minutes_is_end_unknown_never_running() {
    let gap = END_UNKNOWN_AFTER;
    for (now, want) in [
        (T0 + 10 + gap - 1, SubState::Running),
        (T0 + 10 + gap, SubState::EndUnknown),
        (T0 + 10 + 50 * gap, SubState::EndUnknown),
    ] {
        let s = session("log-unknown");
        meta(&s.dir, "a", TEAMMATE);
        rows(&s.dir, "a", &[FIRST, &tick(10)]);
        s.log(&[ev(0, "start", "a")]);
        let sub = s.one(now);
        assert_eq!((sub.state, sub.active_at), (want, Some(T0 + 10)), "now {}", now - T0);
        assert_eq!(sub.finished_at, None, "neither is finished");
    }
}

#[test]
fn active_at_is_the_newest_of_the_newest_row_and_the_last_event() {
    let s = session("log-active");
    meta(&s.dir, "a", PLAIN);
    rows(&s.dir, "a", &[FIRST, &tick(10)]);
    s.log(&[ev(0, "start", "a"), ev(40, "idle", "a")]);
    assert_eq!(s.one(T0 + 60).active_at, Some(T0 + 40));
    s.log(&[ev(0, "start", "a")]);
    assert_eq!(s.one(T0 + 60).active_at, Some(T0 + 10));
}

#[test]
fn a_stop_with_no_start_is_ignored_and_the_meta_decides() {
    // Claude Code's internal agents send a stop with no start; a log cut by the tail can too.
    let s = session("log-orphan");
    meta(&s.dir, "a", TEAMMATE);
    meta(&s.dir, "b", r#"{"agentType":"x","finished":true}"#);
    s.log(&[ev(0, "stop", "a"), ev(1, "idle", "a"), ev(2, "stop", "b")]);
    let mut got: Vec<_> = s.read(T0 + 60).into_iter().map(|x| (x.id, x.state)).collect();
    got.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(got, [("a".to_string(), SubState::Running), ("b".to_string(), SubState::Done)]);
}

/// The fallback is the original #361 bug: a teammate ended by TaskStop or a shutdown never gets
/// `finished`, and with no record in the log (hooks not installed, a session from before them, a
/// start past the tail) it read Running for good.
#[test]
fn an_agent_with_no_record_and_no_write_for_15_minutes_is_end_unknown_not_running() {
    let gap = END_UNKNOWN_AFTER;
    let table: [(&str, &str, bool, u64, SubState); 7] = [
        ("a teammate, quiet for 15 min", TEAMMATE, true, 10 + gap, SubState::EndUnknown),
        ("a subagent, quiet for 15 min", PLAIN, true, 10 + gap, SubState::EndUnknown),
        ("a teammate, one second short", TEAMMATE, true, 10 + gap - 1, SubState::Running),
        (
            "a finished subagent stays done",
            r#"{"agentType":"x","finished":true}"#,
            true,
            10 + 50 * gap,
            SubState::Done,
        ),
        (
            "a stopped one stays stopped",
            r#"{"agentType":"x","stoppedByUser":true}"#,
            true,
            10 + 50 * gap,
            SubState::Stopped,
        ),
        ("no transcript, a meta quiet for 15 min", TEAMMATE, false, gap, SubState::EndUnknown),
        ("no transcript, a fresh meta", TEAMMATE, false, gap - 1, SubState::Running),
    ];
    for (name, m, with_rows, at, want) in table {
        let s = session("log-fallback-aged");
        std::fs::remove_file(&s.log).ok();
        meta(&s.dir, "a", m);
        touch_meta(&s.dir, "a", 0);
        if with_rows {
            rows(&s.dir, "a", &[FIRST, &tick(10)]);
        }
        assert_eq!(s.one(T0 + at).state, want, "{name}");
    }
}

/// `finished` is written when the agent finishes; a log `start` after it is a resume, one before
/// it is an agent whose SubagentStop never came (an API error fires StopFailure, TaskStop fires
/// nothing): the meta file is the newer word.
#[test]
fn a_meta_written_at_or_after_the_last_start_beats_it_and_an_earlier_one_does_not() {
    let err = r#"{"type":"assistant","isApiErrorMessage":true,"timestamp":"2026-11-03T10:01:00Z"}"#;
    let table: [(&str, u64, &str, SubState); 5] = [
        ("meta after the start", 150, "ok", SubState::Done),
        ("meta at the start", 100, "ok", SubState::Done),
        ("meta after the start, an API error row", 150, "err", SubState::Failed),
        ("meta before the start: a resume", 50, "ok", SubState::Running),
        ("meta before the start: a resume, an API error row", 50, "err", SubState::Running),
    ];
    for (name, meta_at, last, want) in table {
        let s = session("log-resume");
        meta(&s.dir, "a", r#"{"agentType":"x","finished":true}"#);
        touch_meta(&s.dir, "a", meta_at);
        let quiet = tick(5);
        rows(&s.dir, "a", &[FIRST, if last == "err" { err } else { &quiet }]);
        s.log(&[ev(100, "start", "a")]);
        assert_eq!(s.one(T0 + 130).state, want, "{name}");
    }
}

#[test]
fn no_log_leaves_the_meta_to_decide() {
    let s = session("log-none");
    std::fs::remove_file(&s.log).ok();
    meta(&s.dir, "a", r#"{"agentType":"x","finished":true}"#);
    meta(&s.dir, "b", PLAIN);
    let mut got: Vec<_> = s.read(T0 + 60).into_iter().map(|x| (x.id, x.state)).collect();
    got.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(got, [("a".to_string(), SubState::Done), ("b".to_string(), SubState::Running)]);
}

#[test]
fn malformed_log_lines_are_skipped() {
    let s = session("log-junk");
    meta(&s.dir, "a", PLAIN);
    let junk = [
        "garbage".to_string(),
        format!("x{T0}\tstop\ta"),
        format!("{T0}\tfoo\ta"),
        format!("{T0}\tstop\ta b"),
        format!("{T0}\tstop"),
        String::new(),
    ];
    let mut lines = vec![ev(0, "start", "a")];
    lines.extend(junk);
    s.log(&lines);
    assert_eq!(s.one(T0 + 60).state, SubState::Running, "none of them is a stop");
}

#[test]
fn the_log_is_read_from_its_last_64_kb_and_a_start_before_it_is_not_seen() {
    let s = session("log-tail");
    meta(&s.dir, "old", PLAIN);
    rows(&s.dir, "old", &[FIRST, &tick(1)]);
    meta(&s.dir, "new", PLAIN);
    let mut lines = vec![ev(0, "start", "old"), ev(1, "stop", "old")];
    // more than 64 KB of another agent's events, then a start and a stop for "new"
    lines.extend((0..4000).map(|i| ev(2 + i % 50, if i % 2 == 0 { "start" } else { "stop" }, "zzzz")));
    lines.extend([ev(100, "start", "new"), ev(101, "stop", "new")]);
    s.log(&lines);
    let mut got: Vec<_> = s.read(T0 + 2000).into_iter().map(|x| (x.id, x.state)).collect();
    got.sort_by(|a, b| a.0.cmp(&b.0));
    // "old" has no record in the window, so its meta decides: no flags and quiet for 33 minutes
    assert_eq!(
        got,
        [("new".to_string(), SubState::Done), ("old".to_string(), SubState::EndUnknown)]
    );
}

#[test]
fn the_log_is_cached_on_its_stamp_and_read_again_only_when_it_changes() {
    let s = session("log-cache");
    meta(&s.dir, "a", PLAIN);
    rows(&s.dir, "a", &[FIRST, &tick(5)]);
    s.log(&[ev(0, "start", "a")]);
    let cold = reads();
    assert_eq!(s.one(T0 + 60).state, SubState::Running);
    assert_eq!(reads() - cold, 3, "the meta file, the transcript and the log");
    let warm = reads();
    s.one(T0 + 61);
    assert_eq!(reads(), warm, "nothing changed: no file is opened");
    s.log(&[ev(0, "start", "a"), ev(9, "stop", "a")]);
    assert_eq!(s.one(T0 + 62).state, SubState::Done);
    assert_eq!(reads() - warm, 1, "only the log grew");
}

#[test]
fn a_session_id_that_is_not_plain_names_no_log() {
    let root = fixture("log-badsid");
    let dir = root.join("bad id").join("subagents");
    std::fs::create_dir_all(&dir).unwrap();
    let subs = root.join("state").join("tmux-cockpit").join("subs");
    std::fs::create_dir_all(&subs).unwrap();
    std::fs::write(subs.join("bad id.log"), ev(0, "start", "a") + "\n").unwrap();
    meta(&dir, "a", r#"{"agentType":"x","finished":true}"#);
    let before = reads();
    let got = read_in(&dir, T0 + 60, &subs);
    assert_eq!((got[0].state, reads() - before), (SubState::Done, 1), "the meta only");
}

#[test]
fn the_state_root_is_xdg_state_home_else_home_local_state() {
    let sub = |r: &str| Some(PathBuf::from(r).join("tmux-cockpit").join("subs"));
    assert_eq!(state_dir(Some("/x"), Some("/h")), sub("/x"));
    assert_eq!(state_dir(Some(""), Some("/h")), sub("/h/.local/state"));
    assert_eq!(state_dir(None, Some("/h")), sub("/h/.local/state"));
    assert_eq!(state_dir(None, None), None);
    assert_eq!(state_dir(Some(""), Some("")), None);
}

#[test]
fn the_hash_ignores_active_at_and_counts_the_new_states() {
    let s = Sub {
        ty: "x".into(),
        state: SubState::Idle,
        ..Default::default()
    };
    assert_eq!(
        h(&s),
        h(&Sub {
            active_at: Some(9),
            ..s.clone()
        })
    );
    for other in [SubState::EndUnknown, SubState::Done, SubState::Running, SubState::Stopped] {
        assert_ne!(h(&s), h(&Sub { state: other, ..s.clone() }), "{other:?}");
    }
}

#[test]
fn a_fork_and_a_teammate_say_so() {
    let d = fixture("kinds");
    meta(&d, "f", r#"{"agentType":"x","isFork":true}"#);
    meta(&d, "t", r#"{"agentType":"x","teamName":"crew"}"#);
    let got: Vec<SubKind> = read(&d).into_iter().map(|s| s.kind).collect();
    assert_eq!(got, [SubKind::Fork, SubKind::Teammate]);
}

#[test]
fn tokens_are_the_newest_usage_row_not_a_sum_over_turns() {
    let d = fixture("tokens");
    meta(&d, "a", r#"{"agentType":"x"}"#);
    rows(&d, "a", &[FIRST, &usage(1, 1, 1, 1), &usage(1000, 200, 30, 4)]);
    assert_eq!(one(&d).tokens, 1234);
}

#[test]
fn usage_fields_that_are_not_numbers_count_zero() {
    let d = fixture("badusage");
    meta(&d, "a", r#"{"agentType":"x"}"#);
    let bad = r#"{"type":"assistant","message":{"usage":{"input_tokens":"9","output_tokens":5}}}"#;
    rows(&d, "a", &[FIRST, bad]);
    assert_eq!(one(&d).tokens, 5);
}

#[test]
fn no_transcript_yet_reads_as_unstarted_with_no_tokens() {
    let d = fixture("nojsonl");
    meta(&d, "a", r#"{"agentType":"x","description":"d"}"#);
    let s = one(&d);
    assert_eq!((s.started, s.tokens, s.state), (None, 0, SubState::Running));
}

#[test]
fn a_meta_that_is_not_a_json_object_or_too_big_is_skipped() {
    let d = fixture("badmeta");
    meta(&d, "n", "not json");
    meta(&d, "a", "[1,2]");
    meta(
        &d,
        "b",
        &format!(r#"{{"agentType":"x","description":"{}"}}"#, "y".repeat(70 * 1024)),
    );
    meta(&d, "ok", r#"{"agentType":"x"}"#);
    let ids: Vec<String> = read(&d).into_iter().map(|s| s.id).collect();
    assert_eq!(ids, ["ok"]);
}

#[test]
fn only_agent_meta_files_with_a_plain_id_are_read() {
    let d = fixture("names");
    meta(&d, "ok", r#"{"agentType":"x"}"#);
    std::fs::write(d.join("foo.meta.json"), r#"{"agentType":"x"}"#).unwrap();
    std::fs::write(d.join("agent-a b.meta.json"), r#"{"agentType":"x"}"#).unwrap();
    std::fs::write(d.join("agent-.meta.json"), r#"{"agentType":"x"}"#).unwrap();
    rows(&d, "lone", &[FIRST]);
    let ids: Vec<String> = read(&d).into_iter().map(|s| s.id).collect();
    assert_eq!(ids, ["ok"]);
}

#[test]
fn strings_lose_control_characters_and_a_wrong_type_reads_empty() {
    let d = fixture("strings");
    meta(&d, "a", r#"{"agentType":"Ex\u001b[31mplore","description":7}"#);
    let s = one(&d);
    assert_eq!((s.ty.as_str(), s.desc.as_str()), ("Ex[31mplore", ""));
}

#[test]
fn an_unchanged_dir_is_served_from_the_cache() {
    let d = fixture("cache");
    meta(&d, "a", r#"{"agentType":"x"}"#);
    rows(&d, "a", &[FIRST, &usage(1, 1, 0, 0)]);
    let first = read(&d);
    let n = reads();
    assert_eq!(read(&d), first);
    assert_eq!(reads(), n, "nothing changed, nothing read");
}

#[test]
fn a_grown_transcript_is_read_again_and_its_meta_is_not() {
    let d = fixture("grow");
    meta(&d, "a", r#"{"agentType":"x"}"#);
    rows(&d, "a", &[FIRST, &usage(1, 1, 0, 0)]);
    read(&d);
    let n = reads();
    rows(&d, "a", &[FIRST, &usage(1, 1, 0, 0), &usage(500, 500, 0, 0)]);
    assert_eq!(one(&d).tokens, 1000);
    assert_eq!(reads() - n, 1, "the transcript once, the meta not at all");
}

#[test]
fn a_removed_subagent_drops_out() {
    let d = fixture("gone");
    meta(&d, "a", r#"{"agentType":"x"}"#);
    meta(&d, "b", r#"{"agentType":"x"}"#);
    assert_eq!(read(&d).len(), 2);
    std::fs::remove_file(d.join("agent-a.meta.json")).unwrap();
    let ids: Vec<String> = read(&d).into_iter().map(|s| s.id).collect();
    assert_eq!(ids, ["b"]);
}

#[test]
fn oldest_first_then_by_id_with_unstarted_last() {
    let d = fixture("order");
    meta(&d, "z", r#"{"agentType":"x"}"#);
    meta(&d, "b", r#"{"agentType":"x"}"#);
    meta(&d, "a", r#"{"agentType":"x"}"#);
    rows(&d, "z", &[FIRST]);
    rows(&d, "b", &[r#"{"timestamp":"2026-11-03T10:00:05Z"}"#]);
    let ids: Vec<String> = read(&d).into_iter().map(|s| s.id).collect();
    assert_eq!(ids, ["z", "b", "a"]);
}

#[test]
fn the_hash_skips_started_and_counts_tokens_by_the_thousand() {
    let s = Sub {
        ty: "x".into(),
        tokens: 1500,
        started: Some(1),
        ..Default::default()
    };
    let rows: [(&str, Sub, bool); 6] = [
        (
            "started moves",
            Sub {
                started: Some(99),
                ..s.clone()
            },
            true,
        ),
        ("1500 to 1999 tokens", Sub { tokens: 1999, ..s.clone() }, true),
        ("2000 tokens", Sub { tokens: 2000, ..s.clone() }, false),
        (
            "done",
            Sub {
                state: SubState::Done,
                ..s.clone()
            },
            false,
        ),
        (
            "a new description",
            Sub {
                desc: "d".into(),
                ..s.clone()
            },
            false,
        ),
        (
            "a new type",
            Sub {
                ty: "y".into(),
                ..s.clone()
            },
            false,
        ),
    ];
    for (name, other, same) in rows {
        assert_eq!(h(&s) == h(&other), same, "{name}");
    }
}

#[test]
fn a_first_row_still_being_written_is_read_again() {
    let d = fixture("partial");
    meta(&d, "a", r#"{"agentType":"x"}"#);
    std::fs::write(d.join("agent-a.jsonl"), &FIRST[..20]).unwrap();
    assert_eq!(one(&d).started, None);
    rows(&d, "a", &[FIRST]);
    assert_eq!(one(&d).started, Some(T0));
}

#[test]
fn a_meta_file_over_64_kb_is_not_even_opened() {
    let d = fixture("huge");
    meta(
        &d,
        "a",
        &format!(r#"{{"agentType":"x","description":"{}"}}"#, "y".repeat(70 * 1024)),
    );
    let n = reads();
    assert!(read(&d).is_empty());
    assert_eq!(reads(), n);
}

#[test]
fn a_panes_hash_follows_its_count_and_its_subs_drawing() {
    use cockpit_tui::data::Pane;
    let hp = |p: &Pane| {
        let mut x = DefaultHasher::new();
        p.hash(&mut x);
        x.finish()
    };
    let s = Sub {
        ty: "x".into(),
        ..Default::default()
    };
    let p = Pane {
        subs: vec![s.clone()],
        ..Default::default()
    };
    let done = Sub {
        state: SubState::Done,
        ..s.clone()
    };
    let later = Sub {
        started: Some(5),
        ..s.clone()
    };
    assert_ne!(hp(&p), hp(&Pane { subs_n: 2, ..p.clone() }), "the hook's count");
    // #301: an ended row draws dimmed, so the redraw gate must see it turn
    assert_ne!(hp(&p), hp(&Pane { ended: true, ..p.clone() }), "ended");
    assert_ne!(
        hp(&p),
        hp(&Pane {
            subs: vec![done],
            ..p.clone()
        }),
        "a sub's state"
    );
    assert_eq!(
        hp(&p),
        hp(&Pane {
            subs: vec![later],
            ..p.clone()
        }),
        "a sub's start"
    );
}

#[test]
fn a_shorter_transcript_was_replaced_and_its_start_is_read_again() {
    let d = fixture("replaced");
    meta(&d, "a", r#"{"agentType":"x"}"#);
    rows(&d, "a", &[FIRST, &usage(1, 1, 0, 0), &usage(2, 2, 0, 0)]);
    assert_eq!(one(&d).started, Some(T0));
    rows(&d, "a", &[r#"{"timestamp":"2026-11-03T10:00:05Z"}"#]);
    assert_eq!(one(&d).started, Some(T0 + 5));
}

#[test]
fn an_empty_team_name_is_no_teammate() {
    let d = fixture("noteam");
    meta(&d, "a", r#"{"agentType":"x","teamName":""}"#);
    assert_eq!(one(&d).kind, SubKind::Subagent);
}

// #403: the TaskStop rows and the Stop reconcile.

const NAMED: &str = r#"{"agentType":"tm1","description":"i1 named","name":"tm1","teamName":"crew"}"#;

/// A `kill-tm` row: the name the model gave TaskStop, and the description as the hook writes
/// it (the JSON string's body).
fn kill_tm(secs: u64, name: &str, desc: &str) -> String {
    format!("{}\tkill-tm\t{name}\t{desc}", T0 + secs)
}

/// A `live` row: the subagents' ids, then one field per teammate description.
fn live(secs: u64, ids: &[&str], descs: &[&str]) -> String {
    let mut l = format!("{}\tlive\t{}", T0 + secs, ids.join(","));
    for d in descs {
        l = format!("{l}\t{d}");
    }
    l
}

#[test]
fn a_plain_agent_stopped_mid_turn_is_stopped_with_no_subagent_stop() {
    let s = session("kill-plain");
    meta(&s.dir, "a", PLAIN);
    rows(&s.dir, "a", &[FIRST, &tick(5)]);
    s.log(&[ev(0, "start", "a"), ev(9, "kill", "a")]);
    assert_eq!(s.one(T0 + 60).state, SubState::Stopped);
    // and never ages into end-unknown
    assert_eq!(s.one(T0 + END_UNKNOWN_AFTER + 60).state, SubState::Stopped);
}

#[test]
fn a_kill_of_another_agent_leaves_this_one_running() {
    let s = session("kill-other");
    meta(&s.dir, "a", PLAIN);
    rows(&s.dir, "a", &[FIRST, &tick(5)]);
    s.log(&[ev(0, "start", "a"), ev(9, "kill", "b")]);
    assert_eq!(s.one(T0 + 60).state, SubState::Running);
}

#[test]
fn a_teammate_stopped_by_name_or_by_description_is_stopped() {
    for (name, row) in [
        ("by name", kill_tm(9, "tm1", "something else")),
        ("by description, the task id given", kill_tm(9, "t0sm6hcnt", "i1 named")),
        ("by description, no name", kill_tm(9, "", "i1 named")),
    ] {
        let s = session("kill-tm");
        meta(&s.dir, "a", NAMED);
        rows(&s.dir, "a", &[FIRST, &tick(5)]);
        // the SubagentStop a teammate's TaskStop also fires does not undo it
        s.log(&[ev(0, "start", "a"), row, ev(10, "stop", "a")]);
        assert_eq!(s.one(T0 + 60).state, SubState::Stopped, "{name}");
    }
}

#[test]
fn a_teammate_stop_by_name_does_not_reach_another_teammate_with_its_description() {
    let s = session("kill-tm-name-wins");
    meta(&s.dir, "a", NAMED);
    meta(
        &s.dir,
        "b",
        r#"{"agentType":"x","description":"i1 named","name":"tm2","teamName":"crew"}"#,
    );
    for id in ["a", "b"] {
        rows(&s.dir, id, &[FIRST, &tick(5)]);
    }
    s.log(&[ev(0, "start", "a"), ev(0, "start", "b"), kill_tm(9, "tm2", "i1 named")]);
    let v = s.read(T0 + 60);
    let st: Vec<(&str, SubState)> = v.iter().map(|x| (x.id.as_str(), x.state)).collect();
    assert_eq!(st, [("a", SubState::Running), ("b", SubState::Stopped)]);
}

#[test]
fn a_teammate_stop_matches_no_plain_agent_and_decodes_its_escapes() {
    let s = session("kill-tm-escapes");
    meta(&s.dir, "a", r#"{"agentType":"x","description":"say \"hi\" é"}"#);
    meta(&s.dir, "b", r#"{"agentType":"x","description":"say \"hi\" é","teamName":"crew"}"#);
    for id in ["a", "b"] {
        rows(&s.dir, id, &[FIRST, &tick(5)]);
    }
    s.log(&[ev(0, "start", "a"), ev(0, "start", "b"), kill_tm(9, "", r#"say \"hi\" é"#)]);
    let v = s.read(T0 + 60);
    let st: Vec<(&str, SubState)> = v.iter().map(|x| (x.id.as_str(), x.state)).collect();
    assert_eq!(st, [("a", SubState::Running), ("b", SubState::Stopped)]);
}

#[test]
fn a_stop_then_a_resume_runs_again() {
    let s = session("kill-resume");
    meta(&s.dir, "a", PLAIN);
    meta(&s.dir, "b", NAMED);
    for id in ["a", "b"] {
        rows(&s.dir, id, &[FIRST, &tick(5)]);
    }
    s.log(&[
        ev(0, "start", "a"),
        ev(0, "start", "b"),
        ev(9, "kill", "a"),
        kill_tm(9, "tm1", "i1 named"),
        ev(20, "start", "a"),
        ev(20, "start", "b"),
    ]);
    let v = s.read(T0 + 60);
    assert!(v.iter().all(|x| x.state == SubState::Running), "{v:?}");
}

#[test]
fn a_reconcile_ends_a_running_agent_it_does_not_list() {
    let s = session("live-drop");
    meta(&s.dir, "a", PLAIN);
    meta(&s.dir, "b", PLAIN);
    meta(&s.dir, "c", NAMED);
    meta(
        &s.dir,
        "d",
        r#"{"agentType":"x","description":"gone mate","name":"tm9","teamName":"crew"}"#,
    );
    for id in ["a", "b", "c", "d"] {
        rows(&s.dir, id, &[FIRST, &tick(5)]);
    }
    s.log(&[
        ev(0, "start", "a"),
        ev(0, "start", "b"),
        ev(0, "start", "c"),
        ev(0, "start", "d"),
        ev(8, "stop", "d"),
        live(30, &["a"], &["i1 named"]),
    ]);
    let v = s.read(T0 + 60);
    let st: Vec<(&str, SubState)> = v.iter().map(|x| (x.id.as_str(), x.state)).collect();
    assert_eq!(
        st,
        [
            ("a", SubState::Running),
            ("b", SubState::Stopped),
            ("c", SubState::Running),
            ("d", SubState::Idle),
        ],
        "a listed by id, c by description; b is not listed; the idle teammate d is left alone"
    );
}

#[test]
fn a_reconcile_leaves_an_agent_started_or_heard_from_after_it() {
    let s = session("live-later");
    meta(&s.dir, "a", PLAIN);
    meta(&s.dir, "b", NAMED);
    for id in ["a", "b"] {
        rows(&s.dir, id, &[FIRST, &tick(5)]);
    }
    // a started in the Stop's own second; b, a teammate, idled after it
    s.log(&[ev(0, "start", "b"), live(30, &[], &[]), ev(30, "start", "a"), ev(31, "idle", "b")]);
    let v = s.read(T0 + 60);
    let st: Vec<(&str, SubState)> = v.iter().map(|x| (x.id.as_str(), x.state)).collect();
    assert_eq!(st, [("a", SubState::Running), ("b", SubState::Idle)]);
}

#[test]
fn a_reconcile_with_no_running_agents_ends_them_all_and_leaves_the_finished_alone() {
    let s = session("live-empty");
    meta(&s.dir, "a", PLAIN);
    meta(&s.dir, "b", PLAIN);
    meta(&s.dir, "c", r#"{"agentType":"x","finished":true}"#);
    for id in ["a", "b", "c"] {
        rows(&s.dir, id, &[FIRST, &tick(5)]);
    }
    touch_meta(&s.dir, "c", 20);
    s.log(&[
        ev(0, "start", "a"),
        ev(0, "start", "b"),
        ev(9, "stop", "b"),
        ev(0, "start", "c"),
        live(30, &[], &[]),
    ]);
    let v = s.read(T0 + 60);
    let st: Vec<(&str, SubState)> = v.iter().map(|x| (x.id.as_str(), x.state)).collect();
    assert_eq!(
        st,
        [("a", SubState::Stopped), ("b", SubState::Done), ("c", SubState::Done)],
        "a running agent ends; a done one stays done; a finished meta still decides"
    );
}

#[test]
fn only_the_newest_reconcile_counts() {
    let s = session("live-newest");
    meta(&s.dir, "a", PLAIN);
    rows(&s.dir, "a", &[FIRST, &tick(5)]);
    s.log(&[ev(0, "start", "a"), live(20, &[], &[]), live(30, &["a"], &[])]);
    assert_eq!(s.one(T0 + 60).state, SubState::Running);
}

#[test]
fn end_unknown_stays_only_when_no_stop_or_reconcile_applies() {
    let s = session("live-unknown");
    meta(&s.dir, "a", PLAIN);
    rows(&s.dir, "a", &[FIRST, &tick(5)]);
    s.log(&[ev(0, "start", "a"), live(30, &["a"], &[])]);
    assert_eq!(s.one(T0 + END_UNKNOWN_AFTER + 60).state, SubState::EndUnknown);
}

#[test]
fn malformed_stop_and_reconcile_rows_are_skipped() {
    let s = session("kill-junk");
    meta(&s.dir, "a", NAMED);
    rows(&s.dir, "a", &[FIRST, &tick(5)]);
    let junk = [
        format!("{T0}\tkill"),
        format!("{T0}\tkill\ta\textra"),
        format!("{T0}\tkill-tm\ttm1"),
        format!("{T0}\tkill-tm\t\t"),
        format!("{T0}\tkill-tm\ttm9\tbad \\x escape"),
        format!("x{T0}\tkill-tm\ttm1\ti1 named"),
        format!("{}\tlive\tb,../x", T0 + 30),
        format!("{}\tlive\tb\tbad \"quote", T0 + 30),
        format!("{}\tlive", T0 + 30),
    ];
    let mut lines = vec![ev(0, "start", "a")];
    lines.extend(junk);
    s.log(&lines);
    assert_eq!(s.one(T0 + 60).state, SubState::Running, "none of them ends it");
}

#[test]
fn two_teammates_with_one_description_are_not_mapped_by_it() {
    let s = session("kill-tm-twins");
    meta(&s.dir, "a", NAMED);
    meta(&s.dir, "b", r#"{"agentType":"x","description":"i1 named","name":"tm2","teamName":"crew"}"#);
    for id in ["a", "b"] {
        rows(&s.dir, id, &[FIRST, &tick(5)]);
    }
    // the model gave the task id, which is no teammate's name
    s.log(&[ev(0, "start", "a"), ev(0, "start", "b"), kill_tm(9, "t0sm6hcnt", "i1 named")]);
    let v = s.read(T0 + END_UNKNOWN_AFTER + 60);
    assert!(v.iter().all(|x| x.state == SubState::EndUnknown), "{v:?}");
}

#[test]
fn a_teammate_stop_with_no_start_in_the_tail_matches_nothing() {
    let s = session("kill-tm-nostart");
    meta(&s.dir, "a", NAMED);
    rows(&s.dir, "a", &[FIRST, &tick(5)]);
    s.log(&[kill_tm(9, "tm1", "i1 named")]);
    assert_eq!(s.one(T0 + 60).state, SubState::Running, "the meta decides");
}

#[test]
fn a_kill_then_a_stop_stays_stopped() {
    let s = session("kill-sticky");
    meta(&s.dir, "a", PLAIN);
    rows(&s.dir, "a", &[FIRST, &tick(5)]);
    s.log(&[ev(0, "start", "a"), ev(9, "kill", "a"), ev(10, "stop", "a"), ev(11, "idle", "a")]);
    assert_eq!(s.one(T0 + 60).state, SubState::Stopped);
}

#[test]
fn a_start_in_the_reconciles_own_second_is_not_ended_by_it() {
    let s = session("live-same-second");
    meta(&s.dir, "a", PLAIN);
    rows(&s.dir, "a", &[FIRST, &tick(5)]);
    s.log(&[ev(30, "start", "a"), live(30, &[], &[])]);
    assert_eq!(s.one(T0 + 60).state, SubState::Running);
}

#[test]
fn an_agent_that_wrote_after_the_reconcile_is_not_ended_by_it() {
    let s = session("live-wrote-after");
    meta(&s.dir, "a", PLAIN);
    rows(&s.dir, "a", &[FIRST, &tick(40)]);
    s.log(&[ev(0, "start", "a"), live(30, &[], &[])]);
    assert_eq!(s.one(T0 + 60).state, SubState::Running);
}

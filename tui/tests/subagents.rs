//! The subagents reader (#313): `<session>/subagents/agent-<id>.meta.json` and the token
//! figure off `agent-<id>.jsonl`. Its own test binary: the reader keeps a thread-local
//! cache and a read counter, and each test runs on a thread of its own.
use cockpit_tui::data::subagents::{Sub, SubKind, SubState, read, reads};
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

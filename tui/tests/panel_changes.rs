//! The what-changed collector (#1000): fixture repos made under the temp dir,
//! never the user's, and a stand-in `gh` started by its full path with the
//! rig's folder first on the PATH its spawn sees. No row reaches a real gh: it
//! would call GitHub as the developer.
mod fake;
use cockpit_tui::panel::changes::{CAP, Changes, Collector, Gh, Item, Kind, More, Note};
use cockpit_tui::panel::changes::{REFRESH_SECS, SEARCHES_PER_MINUTE};
use cockpit_tui::panel::changes::{github_repo, mark_seen, read_seen, repo_key, seen_dir, utc};
use fake::{Scratch, assert_gh_is_the_rigs, rig_path, rigs_gh, write_exec};
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// 2025-11-13T02:13:20Z, the clock every row starts at.
const T0: u64 = 1_763_000_000;

/// A stand-in gh: counts its starts, keeps the PATH and the argv it saw, and
/// prints the file beside it named after its first word.
const GH: &str = r#"#!/bin/sh
echo x >> "$0.started"
printf '%s\n' "$PATH" > "$0.path"
printf '%s\n' "$*" >> "$0.argv"
cat "$0.$1"
"#;

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// A commit with `msg` as its subject, and its full sha.
fn commit(dir: &Path, msg: &str) -> String {
    git(dir, &["commit", "-q", "--allow-empty", "-m", msg]);
    git(dir, &["rev-parse", "HEAD"])
}

struct Rig {
    s: Scratch,
    bin: PathBuf,
    path: OsString,
    clock: Arc<AtomicU64>,
}

impl Rig {
    /// A rig whose gh has `body`, with `[]` for every list it is asked for.
    fn with(tag: &str, body: &str) -> Rig {
        let name = format!("cockpit-changes-{tag}-{}", std::process::id());
        let s = Scratch::fresh(std::env::temp_dir().join(name));
        let bin = s.join("rig/bin");
        std::fs::create_dir_all(&bin).unwrap();
        write_exec(&bin.join("gh"), body);
        for w in ["pr", "issue", "run"] {
            std::fs::write(bin.join(format!("gh.{w}")), "[]").unwrap();
        }
        let path = OsString::from(rig_path(&bin));
        assert_gh_is_the_rigs(&path, &s.join("rig"));
        Rig {
            s,
            bin,
            path,
            clock: Arc::new(AtomicU64::new(T0)),
        }
    }

    fn new(tag: &str) -> Rig {
        Rig::with(tag, GH)
    }

    fn answer(&self, what: &str, json: &str) {
        std::fs::write(self.bin.join(format!("gh.{what}")), json).unwrap();
    }

    fn store(&self) -> PathBuf {
        self.s.join("store")
    }

    fn gh(&self) -> Gh {
        let program = self.bin.join("gh").into_os_string();
        Gh {
            program,
            path: Some(self.path.clone()),
            budget: Duration::from_secs(20),
        }
    }

    fn collector(&self) -> Collector {
        let clock = self.clock.clone();
        Collector::new(self.gh(), self.store(), Box::new(move || clock.load(Ordering::SeqCst)))
    }

    fn at(&self, t: u64) {
        self.clock.store(t, Ordering::SeqCst);
    }

    /// A repo under the rig with one commit; origin on github.com when `origin`.
    fn repo(&self, name: &str, origin: bool) -> PathBuf {
        let d = self.s.join(name);
        std::fs::create_dir_all(&d).unwrap();
        git(&d, &["init", "-q", "-b", "main"]);
        if origin {
            git(&d, &["remote", "add", "origin", &format!("https://github.com/o/{name}.git")]);
        }
        commit(&d, "first");
        d
    }

    fn seen(&self, dir: &Path, head: &str, at: u64) {
        mark_seen(&self.store(), &repo_key(dir.to_str().unwrap()), head, at).unwrap();
    }

    fn starts(&self) -> usize {
        std::fs::read_to_string(self.bin.join("gh.started"))
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }
}

fn collect(c: &mut Collector, dir: &Path) -> Changes {
    c.collect(dir.to_str().unwrap())
}

fn of(c: &Changes, kinds: &[Kind]) -> Vec<(Kind, String, String)> {
    let mine = c.items.iter().filter(|i| kinds.contains(&i.kind));
    mine.map(|i| (i.kind, i.cite.clone(), i.text.clone())).collect()
}

fn commits(c: &Changes) -> Vec<(String, String)> {
    of(c, &[Kind::Commit]).into_iter().map(|(_, cite, text)| (cite, text)).collect()
}

fn github(c: &Changes) -> Vec<(Kind, String, String)> {
    of(c, &[Kind::Pr, Kind::Issue, Kind::Ci])
}

/// The names in `dir`.
fn names(dir: &Path) -> Vec<OsString> {
    std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name()).collect()
}

fn short(sha: &str) -> String {
    sha[..12].to_string()
}

// ---- git ----

#[test]
fn commits_after_last_seen_are_listed_newest_first_by_short_sha() {
    let r = Rig::new("after");
    let d = r.repo("r", false);
    r.seen(&d, &git(&d, &["rev-parse", "HEAD"]), T0);
    let (two, three) = (commit(&d, "two"), commit(&d, "three"));
    let got = collect(&mut r.collector(), &d);
    let want = [(short(&three), "three".to_string()), (short(&two), "two".to_string())];
    assert_eq!(commits(&got), want);
    assert_eq!(got.head.as_deref(), Some(three.as_str()));
}

#[test]
fn nothing_seen_lists_the_newest_commits() {
    let r = Rig::new("unseen");
    let d = r.repo("r", false);
    let two = commit(&d, "two");
    let got = collect(&mut r.collector(), &d);
    assert_eq!(got.seen, None);
    assert_eq!(commits(&got).len(), 2);
    assert_eq!(commits(&got)[0].0, short(&two));
}

#[test]
fn a_last_seen_at_head_lists_no_commit() {
    let r = Rig::new("athead");
    let d = r.repo("r", false);
    r.seen(&d, &commit(&d, "two"), T0);
    let got = collect(&mut r.collector(), &d);
    assert_eq!((commits(&got), got.more), (vec![], vec![]));
}

#[test]
fn commits_past_the_cap_are_counted_as_and_k_more() {
    assert_eq!(CAP, 20);
    let r = Rig::new("cap");
    let d = r.repo("r", false);
    r.seen(&d, &git(&d, &["rev-parse", "HEAD"]), T0);
    let last: Vec<String> = (0..CAP + 3).map(|i| commit(&d, &format!("c{i}"))).collect();
    let got = collect(&mut r.collector(), &d);
    assert_eq!(commits(&got).len(), CAP);
    assert_eq!(commits(&got)[0].0, short(&last[CAP + 2]));
    assert_eq!(got.more, [(Kind::Commit, More::Exactly(3))]);
    assert!(got.lines().contains(&"and 3 more".to_string()), "{:?}", got.lines());
}

#[test]
fn a_last_seen_sha_reset_away_gives_one_rewritten_item() {
    let r = Rig::new("reset");
    let d = r.repo("r", false);
    let one = git(&d, &["rev-parse", "HEAD"]);
    let two = commit(&d, "two");
    r.seen(&d, &two, T0);
    git(&d, &["reset", "-q", "--hard", &one]);
    commit(&d, "other");
    let want = Item {
        kind: Kind::Rewritten,
        cite: short(&two),
        text: "history rewritten since 2025-11-13T02:13:20Z".into(),
    };
    assert_eq!(collect(&mut r.collector(), &d).items, [want]);
}

#[test]
fn a_last_seen_sha_never_in_the_repo_gives_the_rewritten_item_too() {
    let r = Rig::new("absent");
    let d = r.repo("r", false);
    r.seen(&d, &"ab".repeat(20), T0);
    let got = collect(&mut r.collector(), &d);
    assert_eq!(
        of(&got, &[Kind::Rewritten, Kind::Commit]),
        [(Kind::Rewritten, "abababababab".into(), utc_text(T0))]
    );
}

fn utc_text(t: u64) -> String {
    format!("history rewritten since {}", utc(t))
}

#[test]
fn a_secret_shaped_commit_subject_comes_out_redacted() {
    let r = Rig::new("secret");
    let d = r.repo("r", false);
    commit(&d, "rotate sk-ant-MADEUPFAKEFAKE00 today");
    let got = collect(&mut r.collector(), &d);
    assert_eq!(commits(&got)[0].1, "rotate [hidden] today");
}

#[test]
fn a_dir_outside_a_repo_empty_or_relative_gives_only_the_note() {
    let r = Rig::new("norepo");
    std::fs::create_dir_all(r.s.join("plain")).unwrap();
    let plain = r.s.join("plain").display().to_string();
    // "" and "." are the cwd, this checkout's own tui folder, under `git -C`
    for (name, dir) in [("empty", ""), ("dot", "."), ("plain", plain.as_str())] {
        let want = Changes {
            notes: vec![Note::NotARepo],
            ..Changes::default()
        };
        assert_eq!(r.collector().collect(dir), want, "row {name}");
    }
    assert_eq!(r.starts(), 0);
}

#[test]
fn collecting_never_writes_the_last_seen_store() {
    let r = Rig::new("nowrite");
    let d = r.repo("r", true);
    collect(&mut r.collector(), &d);
    assert!(!r.store().exists(), "collect made the store");
    r.seen(&d, &git(&d, &["rev-parse", "HEAD"]), T0);
    let before = std::fs::read(r.store().join(repo_key(d.to_str().unwrap()))).unwrap();
    commit(&d, "two");
    r.at(T0 + 999);
    collect(&mut r.collector(), &d);
    let names = names(&r.store());
    assert_eq!(names, [OsString::from(repo_key(d.to_str().unwrap()))]);
    assert_eq!(std::fs::read(r.store().join(repo_key(d.to_str().unwrap()))).unwrap(), before);
}

// ---- the last-seen file ----

#[test]
fn mark_seen_writes_one_hash_named_file_mode_600_without_path_or_subject() {
    let r = Rig::new("file");
    let d = r.repo("r", false);
    let head = commit(&d, "subject-word-1000");
    r.seen(&d, &head, T0);
    let key = repo_key(d.to_str().unwrap());
    let names = names(&r.store());
    assert_eq!(names, [OsString::from(&key)]);
    let body = std::fs::read_to_string(r.store().join(&key)).unwrap();
    assert_eq!(body, format!("{head} {T0}\n"));
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!((mode(&r.store().join(&key)), mode(&r.store())), (0o600, 0o700));
}

#[test]
fn repo_key_is_fnv1a_of_the_canonical_path_never_the_path() {
    // no such path, so it is hashed as written; the value from an outside FNV-1a
    assert_eq!(repo_key("/sys/absent-1000/a"), "26794caa85a8f535");
    let r = Rig::new("key");
    let d = r.repo("r", false);
    std::os::unix::fs::symlink(&d, r.s.join("link")).unwrap();
    let key = repo_key(d.to_str().unwrap());
    assert_eq!(repo_key(r.s.join("link").to_str().unwrap()), key);
    assert!(key.len() == 16 && !key.contains('/'), "{key}");
}

#[test]
fn a_malformed_or_unreadable_last_seen_file_is_nothing_seen() {
    let r = Rig::new("malformed");
    let d = r.repo("r", false);
    let (key, head) = (repo_key(d.to_str().unwrap()), git(&d, &["rev-parse", "HEAD"]));
    let rows: [(&str, Vec<u8>); 7] = [
        ("empty", vec![]),
        ("no newline", format!("{head} 1").into()),
        ("short sha", b"abc123 1\n".to_vec()),
        ("extra field", format!("{head} 1 2\n").into()),
        ("negative time", format!("{head} -1\n").into()),
        ("upper case", format!("{} 1\n", head.to_uppercase()).into()),
        ("not utf-8", [head.as_bytes(), b" 1\xff\n"].concat()),
    ];
    for (name, bytes) in rows {
        std::fs::create_dir_all(r.store()).unwrap();
        std::fs::write(r.store().join(&key), bytes).unwrap();
        assert_eq!(read_seen(&r.store(), &key), None, "row {name}");
        assert_eq!(commits(&collect(&mut r.collector(), &d)).len(), 1, "row {name}");
    }
}

#[test]
fn a_last_seen_file_that_cannot_be_read_is_nothing_seen() {
    let r = Rig::new("unreadable");
    let d = r.repo("r", false);
    let key = repo_key(d.to_str().unwrap());
    r.seen(&d, &git(&d, &["rev-parse", "HEAD"]), T0);
    let none = std::fs::Permissions::from_mode(0o000);
    std::fs::set_permissions(r.store().join(&key), none).unwrap();
    // root reads a mode 000 file; then this row holds nothing
    let unreadable = std::fs::File::open(r.store().join(&key)).is_err();
    assert!(!unreadable || read_seen(&r.store(), &key).is_none());
    std::fs::remove_file(r.store().join(&key)).unwrap();
    std::fs::create_dir(r.store().join(&key)).unwrap();
    assert_eq!(read_seen(&r.store(), &key), None, "a folder in its place");
    assert_eq!(commits(&collect(&mut r.collector(), &d)).len(), 1);
}

#[test]
fn mark_seen_refuses_a_bad_head_or_key_by_name() {
    let r = Rig::new("refuse");
    let sha = "ab".repeat(20);
    let err = |k: &str, h: &str| mark_seen(&r.store(), k, h, T0).unwrap_err().to_string();
    assert!(
        err("0123456789abcdef", "HEAD").starts_with("head "),
        "{}",
        err("0123456789abcdef", "HEAD")
    );
    assert!(err("../../x", &sha).starts_with("key "), "{}", err("../../x", &sha));
    assert!(err("", &sha).starts_with("key "));
    assert!(!r.store().exists(), "a refused call made the store");
}

/// The child half of the row below.
#[test]
fn seen_dir_child() {
    if std::env::var_os("CHANGES_SEEN_DIR_CHILD").is_none() {
        return;
    }
    assert_eq!(seen_dir(), PathBuf::from("/sys/absent-1000/cache/tmux-cockpit/panel/seen"));
    eprintln!("the-seen-dir-followed-xdg");
}

#[test]
fn seen_dir_is_under_xdg_cache_home() {
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "seen_dir_child", "--nocapture"])
        .env("CHANGES_SEEN_DIR_CHILD", "1")
        .env("XDG_CACHE_HOME", "/sys/absent-1000/cache")
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success() && said.lines().any(|l| l == "the-seen-dir-followed-xdg"),
        "{said}"
    );
}

#[test]
fn utc_gives_known_instants() {
    assert_eq!(utc(0), "1970-01-01T00:00:00Z");
    assert_eq!(utc(951_782_400), "2000-02-29T00:00:00Z");
    assert_eq!(utc(T0), "2025-11-13T02:13:20Z");
    assert_eq!(utc(4_102_444_799), "2099-12-31T23:59:59Z");
}

// ---- gh ----

#[test]
fn a_gh_outside_the_rig_and_ahead_of_it_is_named() {
    let r = Rig::new("walk");
    let out = r.s.join("out");
    std::fs::create_dir_all(&out).unwrap();
    write_exec(&out.join("gh"), "#!/bin/sh\nexit 1\n");
    let ahead = std::env::join_paths([&out, &r.bin]).unwrap();
    assert_eq!(rigs_gh(&ahead, &r.s.join("rig")), Err(Some(out.join("gh"))));
    assert_eq!(rigs_gh(&r.path, &r.s.join("rig")), Ok(r.bin.join("gh")));
}

/// #977's walk shapes, for gh: an entry that climbs out of the rig, a link in
/// the rig to a gh outside, and a gh file this user may not execute.
#[test]
fn the_gh_walk_judges_a_climb_out_a_link_and_a_dead_file_as_a_spawn_would() {
    let r = Rig::new("walkrows");
    let (rig, out) = (r.s.join("rig"), r.s.join("out"));
    std::fs::create_dir_all(&out).unwrap();
    write_exec(&out.join("gh"), "#!/bin/sh\nexit 1\n");
    let (link, dead) = (rig.join("link"), rig.join("dead"));
    std::fs::create_dir_all(&link).unwrap();
    std::os::unix::fs::symlink(out.join("gh"), link.join("gh")).unwrap();
    std::fs::create_dir_all(&dead).unwrap();
    write_exec(&dead.join("gh"), "#!/bin/sh\nexit 1\n");
    std::fs::set_permissions(dead.join("gh"), std::fs::Permissions::from_mode(0o644)).unwrap();
    let rows = [
        ("climb out", vec![rig.join("../out")], Err(Some(rig.join("../out/gh")))),
        ("link out", vec![link.clone()], Err(Some(link.join("gh")))),
        ("not executable", vec![dead, r.bin.clone()], Ok(r.bin.join("gh"))),
    ];
    for (name, dirs, want) in rows {
        assert_eq!(rigs_gh(&std::env::join_paths(dirs).unwrap(), &rig), want, "row {name}");
    }
}

/// The child half of the row below: its cwd holds a `gh`, its PATH value
/// opens with an empty entry.
#[test]
fn gh_empty_entry_child() {
    let Some(rig) = std::env::var_os("CHANGES_GH_EMPTY_ENTRY") else {
        return;
    };
    let rig = PathBuf::from(rig);
    let mut p = OsString::from(":");
    p.push(rig.join("bin"));
    assert_eq!(rigs_gh(&p, &rig), Err(Some(PathBuf::from("./gh"))));
    eprintln!("the-empty-entry-was-the-cwd");
}

#[test]
fn an_empty_path_entry_is_the_cwd_for_gh_too() {
    let r = Rig::new("emptyentry");
    let cwd = r.s.join("cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    write_exec(&cwd.join("gh"), "#!/bin/sh\nexit 1\n");
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "gh_empty_entry_child", "--nocapture"])
        .env("CHANGES_GH_EMPTY_ENTRY", r.s.join("rig"))
        .current_dir(cwd)
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stderr);
    let ran = said.lines().any(|l| l == "the-empty-entry-was-the-cwd");
    assert!(out.status.success() && ran, "{said}");
}

#[test]
#[should_panic(expected = "the first gh on PATH is Some(")]
fn assert_gh_is_the_rigs_names_a_gh_ahead_of_the_rigs() {
    let r = Rig::new("assertgh");
    let out = r.s.join("out");
    std::fs::create_dir_all(&out).unwrap();
    write_exec(&out.join("gh"), "#!/bin/sh\nexit 1\n");
    let ahead = std::env::join_paths([&out, &r.bin]).unwrap();
    assert_gh_is_the_rigs(&ahead, &r.s.join("rig"));
}

#[test]
fn the_spawn_finds_the_rigs_gh_first_on_the_path_it_sees() {
    let r = Rig::new("path");
    let d = r.repo("r", true);
    collect(&mut r.collector(), &d);
    let seen = std::fs::read_to_string(r.bin.join("gh.path")).unwrap();
    let seen = OsString::from(seen.trim_end_matches('\n'));
    assert_eq!(seen, r.path);
    assert_eq!(rigs_gh(&seen, &r.s.join("rig")), Ok(r.bin.join("gh")));
}

#[test]
fn pr_issue_and_ci_items_cite_their_numbers_newest_first() {
    let r = Rig::new("items");
    let d = r.repo("r", true);
    let pr = r#"[{"number":12,"title":"Older","updatedAt":"2026-11-13T09:00:00Z"},
        {"number":15,"title":"Newer","updatedAt":"2026-11-13T10:00:00Z"}]"#;
    r.answer("pr", pr);
    r.answer("issue", r#"[{"number":7,"title":"A bug","updatedAt":"2026-11-13T08:00:00Z"}]"#);
    let run = r#"[{"databaseId":123456789,"workflowName":"ci","status":"completed","#;
    r.answer("run", &format!(r#"{run}"conclusion":"success"}}]"#));
    let got = collect(&mut r.collector(), &d);
    let want = [
        (Kind::Pr, "#15", "Newer"),
        (Kind::Pr, "#12", "Older"),
        (Kind::Issue, "#7", "A bug"),
        (Kind::Ci, "123456789", "ci: success"),
    ];
    assert_eq!(github(&got), want.map(|(k, c, t)| (k, c.to_string(), t.to_string())));
    assert_eq!((r.starts(), got.notes), (3, vec![]));
}

#[test]
fn gh_is_asked_for_the_repo_its_branch_and_the_time_since_last_seen() {
    let r = Rig::new("argv");
    let d = r.repo("r", true);
    r.seen(&d, &git(&d, &["rev-parse", "HEAD"]), T0);
    collect(&mut r.collector(), &d);
    let argv = std::fs::read_to_string(r.bin.join("gh.argv")).unwrap();
    let mut argv: Vec<&str> = argv.lines().collect();
    argv.sort();
    let search = "--search updated:>=2025-11-13T02:13:20Z sort:updated-desc";
    let tail = "--json number,title,updatedAt -R github.com/o/r";
    let list = |w| format!("{w} list --state all --limit 100 {search} {tail}");
    let fields = "databaseId,workflowName,status,conclusion";
    let run = format!("run list --branch main --limit 1 --json {fields} -R github.com/o/r");
    assert_eq!(argv, [list("issue").as_str(), list("pr").as_str(), &run]);
}

#[test]
fn uncited_prs_issues_and_runs_are_dropped() {
    let r = Rig::new("uncited");
    let d = r.repo("r", true);
    let pr = r#"[{"title":"no number"},{"number":0,"title":"zero"},
        {"number":"9","title":"a string"},
        {"number":5,"title":"kept"}]"#;
    r.answer("pr", pr);
    r.answer("issue", r#"[{"number":-3,"title":"negative"},{"number":1.5,"title":"a fraction"}]"#);
    r.answer("run", r#"[{"workflowName":"ci","status":"queued"}]"#);
    let got = collect(&mut r.collector(), &d);
    assert_eq!(github(&got), [(Kind::Pr, "#5".to_string(), "kept".to_string())]);
}

#[test]
fn gh_lists_past_the_cap_are_counted() {
    let r = Rig::new("ghcap");
    let d = r.repo("r", true);
    let rows = |n: usize| {
        (1..=n)
            .map(|i| format!(r#"{{"number":{i},"title":"t{i}"}}"#))
            .collect::<Vec<_>>()
            .join(",")
    };
    r.answer("pr", &format!("[{}]", rows(CAP + 5)));
    r.answer("issue", &format!("[{}]", rows(100)));
    let got = collect(&mut r.collector(), &d);
    assert_eq!(of(&got, &[Kind::Pr]).len(), CAP);
    assert_eq!(got.more, [(Kind::Pr, More::Exactly(5)), (Kind::Issue, More::AtLeast(80))]);
}

#[test]
fn a_pr_title_with_a_bidi_control_or_a_secret_comes_out_clean() {
    let r = Rig::new("title");
    let d = r.repo("r", true);
    // the JSON escape, so this file holds no bidi control itself
    let pr = r#"[{"number":3,"title":"fix \u202Etxt.exe"},
        {"number":4,"title":"leak ghp_MADEUPFAKEFAKEFAKEFAKE00 here"}]"#;
    r.answer("pr", pr);
    let got = collect(&mut r.collector(), &d);
    let texts: Vec<String> = github(&got).into_iter().map(|(_, _, t)| t).collect();
    assert_eq!(texts, ["fix txt.exe", "leak [hidden] here"]);
}

#[test]
fn the_floor_keeps_a_refresh_for_60_seconds() {
    assert_eq!(REFRESH_SECS, 60);
    let r = Rig::new("floor");
    let d = r.repo("r", true);
    r.answer("pr", r#"[{"number":1,"title":"one"}]"#);
    let mut c = r.collector();
    let first = collect(&mut c, &d);
    r.at(T0 + 59);
    assert_eq!((collect(&mut c, &d), r.starts()), (first, 3));
    r.at(T0 + 60);
    collect(&mut c, &d);
    assert_eq!(r.starts(), 6);
}

#[test]
fn a_new_last_seen_inside_the_floor_gives_the_floor_note() {
    let r = Rig::new("floornote");
    let d = r.repo("r", true);
    r.answer("pr", r#"[{"number":1,"title":"one"}]"#);
    let mut c = r.collector();
    collect(&mut c, &d);
    r.seen(&d, &git(&d, &["rev-parse", "HEAD"]), T0 + 1);
    r.at(T0 + 10);
    let got = collect(&mut c, &d);
    assert_eq!(
        (github(&got), got.notes, r.starts()),
        (vec![], vec![Note::Floor { again_in: 50 }], 3)
    );
}

#[test]
fn the_30_searches_a_minute_hold_across_repos() {
    assert_eq!(SEARCHES_PER_MINUTE, 30);
    let r = Rig::new("searches");
    let mut c = r.collector();
    for i in 0..15 {
        collect(&mut c, &r.repo(&format!("r{i}"), true));
    }
    assert_eq!(r.starts(), 45);
    let last = r.repo("r15", true);
    assert_eq!(collect(&mut c, &last).notes, [Note::SearchLimit { again_in: 60 }]);
    assert_eq!(r.starts(), 45);
    r.at(T0 + 60);
    assert_eq!((collect(&mut c, &last).notes, r.starts()), (vec![], 48));
}

#[test]
fn an_origin_off_github_starts_no_gh() {
    let r = Rig::new("offgh");
    let none = r.repo("none", false);
    let lab = r.repo("lab", false);
    git(&lab, &["remote", "add", "origin", "https://gitlab.com/o/lab.git"]);
    for d in [none, lab] {
        assert_eq!(collect(&mut r.collector(), &d).notes, [Note::NoGithubRemote]);
    }
    assert_eq!(r.starts(), 0);
}

#[test]
fn github_repo_reads_https_ssh_and_scp_forms_only_for_github() {
    let rows = [
        ("https", "https://github.com/o/r.git", Some("o/r")),
        ("no .git", "https://github.com/o/r", Some("o/r")),
        ("ssh", "ssh://git@github.com/o-1/r.x.git", Some("o-1/r.x")),
        ("scp", "git@github.com:o/r.git\n", Some("o/r")),
        ("other host", "https://gitlab.com/o/r.git", None),
        ("deeper", "https://github.com/o/r/x", None),
        ("dot dot", "https://github.com/../r", None),
        ("a flag", "git@github.com:-o/r", None),
        ("empty", "", None),
    ];
    for (name, url, want) in rows {
        assert_eq!(github_repo(url).as_deref(), want, "row {name}");
    }
}

// ---- failing closed ----

/// The gh failures, each with the one note it leaves: a stand-in body, or
/// `None` for no gh on PATH at all.
fn failures() -> [(&'static str, Option<&'static str>, Note); 6] {
    [
        ("no gh", None, Note::NoGh),
        (
            "not logged in",
            Some("#!/bin/sh\necho 'To get started with GitHub CLI' >&2\nexit 4\n"),
            Note::NotLoggedIn,
        ),
        ("silent", Some("#!/bin/sh\nexit 0\n"), Note::GhSilent),
        ("not json", Some("#!/bin/sh\necho hello\n"), Note::GhUnreadable),
        ("wrong json", Some("#!/bin/sh\necho '{\"message\":\"x\"}'\n"), Note::GhUnreadable),
        ("failed", Some("#!/bin/sh\nexit 1\n"), Note::GhFailed),
    ]
}

/// What `body` (or no gh) leaves; the commits still listed.
fn failed_with(tag: &str, body: Option<&str>) -> Changes {
    let r = Rig::with(tag, body.unwrap_or(GH));
    let d = r.repo("r", true);
    let mut gh = r.gh();
    if body.is_none() {
        let empty = r.s.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert_eq!(rigs_gh(empty.as_os_str(), &r.s), Err(None), "a gh on the empty PATH");
        gh = Gh {
            program: "gh".into(),
            path: Some(empty.into_os_string()),
            ..gh
        };
    }
    let mut c = Collector::new(gh, r.store(), Box::new(|| T0));
    collect(&mut c, &d)
}

#[test]
fn each_gh_failure_leaves_one_note_no_items_and_the_commits() {
    for (name, body, note) in failures() {
        let got = failed_with(&name.replace(' ', "-"), body);
        assert_eq!((github(&got), got.notes.clone()), (vec![], vec![note]), "row {name}");
        assert_eq!(commits(&got).len(), 1, "row {name}");
    }
}

/// A gh that answers in no budget a row sets and leaves a child in its group.
/// A line in `gh.done` says its own wait ended, which a killed shell never says.
const HANGS: &str = "#!/bin/sh\nsleep 20 &\necho $! >> \"$0.kids\"\nwait\necho x >> \"$0.done\"\n";

/// `pid` is gone or a zombie, polled at most 2000 times.
fn dies(pid: &str) -> bool {
    let gone = || match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => stat.rsplit(") ").next().is_some_and(|t| t.starts_with('Z')),
        Err(_) => true,
    };
    for _ in 0..2000 {
        if gone() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

#[test]
fn a_gh_past_its_budget_is_killed_with_its_group() {
    let r = Rig::with("hang", HANGS);
    let d = r.repo("r", true);
    let gh = Gh {
        budget: Duration::from_millis(300),
        ..r.gh()
    };
    let got = collect(&mut Collector::new(gh, r.store(), Box::new(|| T0)), &d);
    assert_eq!((github(&got), got.notes.clone()), (vec![], vec![Note::GhTimedOut]));
    let kids = std::fs::read_to_string(r.bin.join("gh.kids")).unwrap();
    let alive: Vec<&str> = kids.lines().filter(|p| !dies(p)).collect();
    for p in &alive {
        // only a sleep this row started, checked by name before the kill
        if std::fs::read_to_string(format!("/proc/{p}/comm")).is_ok_and(|c| c.trim() == "sleep") {
            let _ = Command::new("kill").arg(p).status();
        }
    }
    assert_eq!((kids.lines().count(), alive), (3, vec![]));
    assert!(!r.bin.join("gh.done").exists(), "a gh shell outlived its budget");
}

/// The child half of the row below: every failure, the hang included, then a marker.
#[test]
fn quiet_child() {
    if std::env::var_os("CHANGES_QUIET_CHILD").is_none() {
        return;
    }
    for (name, body, _) in failures() {
        failed_with(&format!("quiet-{}", name.replace(' ', "-")), body);
    }
    let r = Rig::with("quiet-hang", HANGS);
    let d = r.repo("r", true);
    let gh = Gh {
        budget: Duration::from_millis(300),
        ..r.gh()
    };
    Collector::new(gh, r.store(), Box::new(|| T0)).collect(d.to_str().unwrap());
    eprintln!("the-failures-were-quiet");
}

#[test]
fn no_failure_prints_a_line() {
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "quiet_child", "--nocapture"])
        .env("CHANGES_QUIET_CHILD", "1")
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{said}");
    assert_eq!(said.lines().collect::<Vec<_>>(), ["the-failures-were-quiet"]);
}

// ---- review fixes: a branch switch is no rewrite ----

#[test]
fn a_last_seen_sha_on_another_branch_is_no_rewrite_after_a_checkout() {
    let r = Rig::new("switch");
    let d = r.repo("r", false);
    git(&d, &["checkout", "-q", "-b", "feat"]);
    r.seen(&d, &commit(&d, "on feat"), T0);
    git(&d, &["checkout", "-q", "main"]);
    let main = commit(&d, "on main");
    let got = collect(&mut r.collector(), &d);
    let want = [(Kind::Commit, short(&main), "on main".to_string())];
    assert_eq!(of(&got, &[Kind::Rewritten, Kind::Commit]), want);
}

#[test]
fn a_last_seen_tip_after_checking_out_its_parent_is_no_rewrite() {
    let r = Rig::new("parent");
    let d = r.repo("r", false);
    r.seen(&d, &commit(&d, "two"), T0);
    git(&d, &["checkout", "-q", "--detach", "HEAD~1"]);
    let got = collect(&mut r.collector(), &d);
    assert_eq!((got.items, got.notes), (vec![], vec![Note::NoGithubRemote]));
}

#[test]
fn a_last_seen_sha_dropped_with_its_branch_is_still_a_rewrite() {
    let r = Rig::new("dropped");
    let d = r.repo("r", false);
    git(&d, &["checkout", "-q", "-b", "feat"]);
    let gone = commit(&d, "on feat");
    r.seen(&d, &gone, T0);
    git(&d, &["checkout", "-q", "main"]);
    git(&d, &["branch", "-q", "-D", "feat"]);
    commit(&d, "on main");
    let got = collect(&mut r.collector(), &d);
    let want = [(Kind::Rewritten, short(&gone), utc_text(T0))];
    assert_eq!(of(&got, &[Kind::Rewritten, Kind::Commit]), want);
}

// ---- review fixes: the cut comes after redaction ----

/// A made-up key from char 180 to 208, across the 200-char cut.
#[test]
fn a_key_across_the_200_char_cut_comes_out_hidden() {
    let r = Rig::new("straddle");
    let d = r.repo("r", true);
    let lead = "a".repeat(179);
    let title = format!("{lead} ghp_MADEUPFAKEFAKEFAKEFAKE00 tail");
    r.answer("pr", &format!(r#"[{{"number":2,"title":"{title}"}}]"#));
    let got = collect(&mut r.collector(), &d);
    assert_eq!(github(&got)[0].2, format!("{lead} [hidden] tail"));
}

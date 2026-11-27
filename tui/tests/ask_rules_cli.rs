//! `cockpit-tui ask-rules check|accept` (#1508, epic #1500): the owner's rules file,
//! loaded and checked so that any doubt turns a rule off. Each row runs the binary in a
//! folder of its own, with HOME and both XDG homes pointing into it, so no row reads the
//! owner's real rules and none writes the process environment.
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// A home of its own: `c/` is XDG_CONFIG_HOME, `s/` is XDG_STATE_HOME.
struct Home(PathBuf);

impl Home {
    fn new(name: &str) -> Home {
        let d = std::env::temp_dir().join(format!("ask-rules-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("c/tmux-cockpit")).unwrap();
        fs::create_dir_all(d.join("s/tmux-cockpit")).unwrap();
        Home(d)
    }

    fn rules(&self) -> PathBuf {
        self.0.join("c/tmux-cockpit/ask-rules.json")
    }

    fn copy(&self) -> PathBuf {
        self.0.join("s/tmux-cockpit/ask-rules.accepted.json")
    }

    fn write(&self, p: &PathBuf, text: &str, mode: u32) {
        fs::write(p, text).unwrap();
        fs::set_permissions(p, fs::Permissions::from_mode(mode)).unwrap();
    }

    /// The rules file, 0600, and the same bytes as the accepted copy.
    fn accepted(&self, text: &str) {
        self.write(&self.rules(), text, 0o600);
        self.write(&self.copy(), text, 0o600);
    }

    /// The exit code and stdout of `cockpit-tui ask-rules <args>`, with `stdin` piped in.
    fn run(&self, args: &[&str], stdin: &str) -> (i32, String) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_cockpit-tui"))
            .arg("ask-rules")
            .args(args)
            .env_clear()
            .env("HOME", &self.0)
            .env("XDG_CONFIG_HOME", self.0.join("c"))
            .env("XDG_STATE_HOME", self.0.join("s"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // A child that refuses before it reads exits first: a broken pipe here is no failure.
        let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
        let out = child.wait_with_output().unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        (out.status.code().unwrap_or(-1), text)
    }

    fn check(&self) -> (i32, String) {
        self.run(&["check"], "")
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// One rule as the file holds it.
fn rule(id: &str, when: &str, examples: &str) -> String {
    format!(r#"{{"id": "{id}", "action": "merge", "when": [{when}], "mode": "shadow", "examples": {examples}}}"#)
}

fn file(rules: &[String]) -> String {
    format!("{{\"schema_version\": 1, \"rules\": [\n{}\n]}}\n", rules.join(",\n"))
}

/// A rule that loads: merge when the verdict is at the head, with one example each way.
fn good(id: &str) -> String {
    rule(
        id,
        r#""verdict_merge_at_head""#,
        r#"{"match": [{"verdict_merge_at_head": true}], "no_match": [{"verdict_merge_at_head": false}]}"#,
    )
}

#[test]
fn an_unknown_predicate_turns_that_rule_off_and_spares_its_neighbour() {
    let h = Home::new("pred");
    let odd = rule(
        "odd",
        r#""verdict_merge_at_head", "moon_is_full""#,
        r#"{"match": [{"verdict_merge_at_head": true}], "no_match": [{"verdict_merge_at_head": false}]}"#,
    );
    h.accepted(&file(&[good("fine"), odd]));
    let (code, out) = h.check();
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("fine on shadow merge"), "{out}");
    assert!(out.contains("odd off: unknown predicate moon_is_full"), "{out}");
}

#[test]
fn a_failing_match_example_turns_the_rule_off() {
    let h = Home::new("match");
    let r = rule(
        "two",
        r#""verdict_merge_at_head", "ci_green_on_merge""#,
        r#"{"match": [{"verdict_merge_at_head": true}], "no_match": [{"verdict_merge_at_head": false}]}"#,
    );
    h.accepted(&file(&[r]));
    let (code, out) = h.check();
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("two off: match example 0 fails"), "{out}");
}

#[test]
fn a_failing_no_match_example_turns_the_rule_off() {
    let h = Home::new("nomatch");
    let r = rule(
        "loose",
        r#""verdict_merge_at_head""#,
        r#"{"match": [{"verdict_merge_at_head": true}], "no_match": [{"verdict_merge_at_head": true, "ci_green_on_merge": false}]}"#,
    );
    h.accepted(&file(&[r]));
    let (code, out) = h.check();
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("loose off: no_match example 0 fails"), "{out}");
}

#[test]
fn an_edit_after_accept_turns_every_rule_off() {
    let h = Home::new("edit");
    let text = file(&[good("a"), good("b")]);
    h.accepted(&text);
    let (code, out) = h.check();
    assert_eq!(code, 0, "accepted as is: {out}");
    h.write(&h.rules(), &format!("{text} "), 0o600);
    let (code, out) = h.check();
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("off: changed since it was accepted"), "{out}");
    assert!(!out.contains(" on shadow"), "{out}");
}

#[test]
fn with_no_accepted_copy_every_rule_is_off() {
    let h = Home::new("nocopy");
    h.write(&h.rules(), &file(&[good("a")]), 0o600);
    let (code, out) = h.check();
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("off: never accepted"), "{out}");
    assert!(!out.contains(" on shadow"), "{out}");
}

#[test]
fn a_rules_file_or_copy_that_is_not_0600_is_refused() {
    let h = Home::new("mode");
    let text = file(&[good("a")]);
    h.accepted(&text);
    fs::set_permissions(h.rules(), fs::Permissions::from_mode(0o644)).unwrap();
    let (code, out) = h.check();
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("the rules file is not mode 0600"), "{out}");
    fs::set_permissions(h.rules(), fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(h.copy(), fs::Permissions::from_mode(0o644)).unwrap();
    let (code, out) = h.check();
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("the accepted copy is not mode 0600"), "{out}");
}

#[test]
fn a_symlinked_rules_file_is_refused() {
    let h = Home::new("link");
    let text = file(&[good("a")]);
    let real = h.0.join("real.json");
    h.write(&real, &text, 0o600);
    h.write(&h.copy(), &text, 0o600);
    std::os::unix::fs::symlink(&real, h.rules()).unwrap();
    let (code, out) = h.check();
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("the rules file is a symlink"), "{out}");
}

#[test]
fn an_unknown_key_refuses_the_whole_file() {
    let h = Home::new("key");
    let odd = good("b").replacen("{", r#"{"colour": "red", "#, 1);
    h.accepted(&file(&[good("a"), odd]));
    let (code, out) = h.check();
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("unknown key rules[1].colour"), "{out}");
    assert!(!out.contains(" on shadow"), "{out}");
}

#[test]
fn accept_shows_the_diff_and_copies_only_on_yes() {
    let h = Home::new("accept");
    let text = file(&[good("first")]);
    h.write(&h.rules(), &text, 0o600);
    let (code, out) = h.run(&["accept"], "n\n");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains(r#"+{"id": "first""#), "the diff: {out}");
    assert!(!h.copy().exists(), "a no wrote the copy: {out}");
    let (code, out) = h.run(&["accept"], "y\n");
    assert_eq!(code, 0, "{out}");
    assert_eq!(fs::read_to_string(h.copy()).unwrap(), text);
    let mode = fs::metadata(h.copy()).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let (code, out) = h.check();
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("first on shadow merge"), "{out}");
}

#[test]
fn accept_refuses_a_file_with_a_rule_off() {
    let h = Home::new("refuse");
    let odd = rule(
        "odd",
        r#""moon_is_full""#,
        r#"{"match": [{"verdict_merge_at_head": true}], "no_match": [{"verdict_merge_at_head": false}]}"#,
    );
    h.write(&h.rules(), &file(&[good("a"), odd]), 0o600);
    let (code, out) = h.run(&["accept"], "y\n");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("odd"), "{out}");
    assert!(!h.copy().exists(), "an invalid file was accepted: {out}");
}

#[test]
fn cap_per_hour_outside_1_to_60_turns_the_rule_off() {
    let h = Home::new("cap");
    let zero = good("zero").replacen("{", r#"{"cap_per_hour": 0, "#, 1);
    let many = good("many").replacen("{", r#"{"cap_per_hour": 61, "#, 1);
    let one = good("one").replacen("{", r#"{"cap_per_hour": 1, "#, 1);
    h.accepted(&file(&[zero, many, one, good("plain")]));
    let (code, out) = h.check();
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("zero off: cap_per_hour must be 1 to 60"), "{out}");
    assert!(out.contains("many off: cap_per_hour must be 1 to 60"), "{out}");
    assert!(out.contains("one on shadow merge"), "{out}");
    assert!(out.contains("cap 1/h"), "{out}");
    assert!(out.contains("plain on shadow merge"), "{out}");
    assert!(out.contains("cap 3/h"), "an absent cap reads as 3: {out}");
}

#[test]
fn the_shipped_defaults_accept_as_three_shadow_rules() {
    let h = Home::new("defaults");
    let shipped = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/ask-rules.json");
    h.write(&h.rules(), &fs::read_to_string(shipped).unwrap(), 0o600);
    let (code, out) = h.run(&["accept"], "y\n");
    assert_eq!(code, 0, "{out}");
    let (code, out) = h.check();
    assert_eq!(code, 0, "{out}");
    for id in ["merge-after-review", "cleanup-merged-lane", "continue-queue"] {
        assert!(out.contains(&format!("{id} on shadow")), "{id}: {out}");
    }
    assert_eq!(out.matches(" on shadow").count(), 3, "{out}");
}

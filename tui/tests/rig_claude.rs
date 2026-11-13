//! The `claude` a rig answers a lookup with (#972): the rows of the helpers in
//! `fake`. Nothing here sets a variable: every PATH is a value handed to the
//! helper, or the environment of a child.
//!
//! Whether a rig reaches a lookup without that `claude` is not read off its
//! source. The run puts a `claude` that refuses first on every test binary's
//! PATH and fails the binary that started it (#973, `claude_guard.rs`).
mod fake;
use fake::{Scratch, rig_path, rigs_claude};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

fn scratch(tag: &str) -> Scratch {
    let name = format!("cockpit-rigclaude-{tag}-{}", std::process::id());
    Scratch::fresh(std::env::temp_dir().join(name))
}

/// An executable `claude` in `dir/name`, never run by a row that only walks.
fn plant(dir: &Path, name: &str) -> PathBuf {
    let bin = dir.join(name);
    std::fs::create_dir_all(&bin).unwrap();
    fake::write_exec(&bin.join("claude"), "#!/bin/sh\nexit 1\n");
    bin
}

fn path(dirs: &[&Path]) -> OsString {
    std::env::join_paths(dirs).unwrap()
}

#[test]
fn a_claude_outside_the_rig_and_ahead_of_it_is_named() {
    let s = scratch("outside");
    let (out, rig) = (plant(&s, "out"), plant(&s, "rig/bin"));
    assert_eq!(rigs_claude(&path(&[&out, &rig]), &s.join("rig")), Err(Some(out.join("claude"))));
}

#[test]
fn no_claude_anywhere_is_not_the_rigs() {
    let s = scratch("none");
    std::fs::create_dir_all(s.join("empty")).unwrap();
    assert_eq!(rigs_claude(&path(&[&s.join("empty")]), &s.join("rig")), Err(None));
}

#[test]
fn the_rigs_claude_ahead_of_one_outside_is_the_answer() {
    let s = scratch("first");
    let (out, rig) = (plant(&s, "out"), plant(&s, "rig/bin"));
    assert_eq!(rigs_claude(&path(&[&rig, &out]), &s.join("rig")), Ok(rig.join("claude")));
}

#[test]
fn a_claude_file_without_the_exec_bit_is_walked_past() {
    use std::os::unix::fs::PermissionsExt;
    let s = scratch("noexec");
    let (dead, rig) = (plant(&s, "dead"), plant(&s, "rig/bin"));
    std::fs::set_permissions(dead.join("claude"), std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(rigs_claude(&path(&[&dead, &rig]), &s.join("rig")), Ok(rig.join("claude")));
}

#[test]
fn a_folder_named_claude_is_walked_past() {
    let s = scratch("folder");
    let rig = plant(&s, "rig/bin");
    std::fs::create_dir_all(s.join("dir/claude")).unwrap();
    let p = path(&[&s.join("dir"), &rig]);
    assert_eq!(rigs_claude(&p, &s.join("rig")), Ok(rig.join("claude")));
}

/// The child half of the row below: its cwd holds a `claude`, its PATH value
/// opens with an empty entry.
#[test]
fn empty_entry_child() {
    let Some(rig) = std::env::var_os("RIGCLAUDE_EMPTY_ENTRY") else {
        return;
    };
    let rig = PathBuf::from(rig);
    let mut p = OsString::from(":");
    p.push(rig.join("bin"));
    assert_eq!(rigs_claude(&p, &rig), Err(Some(PathBuf::from("./claude"))));
    assert_eq!(rigs_claude(&OsString::new(), &rig), Err(Some(PathBuf::from("./claude"))));
    eprintln!("the-empty-entry-was-the-cwd");
}

#[test]
fn an_empty_entry_is_the_cwd_as_a_spawn_reads_it() {
    let s = scratch("cwd");
    let (cwd, rig) = (plant(&s, "cwd"), plant(&s, "rig/bin"));
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "empty_entry_child", "--nocapture"])
        .env("RIGCLAUDE_EMPTY_ENTRY", rig.parent().unwrap())
        .current_dir(cwd)
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success() && said.lines().any(|l| l == "the-empty-entry-was-the-cwd"),
        "{said}"
    );
}

#[test]
fn an_empty_path_value_with_no_claude_in_the_cwd_finds_none() {
    let s = scratch("emptyvalue");
    assert!(!Path::new("claude").exists(), "the cwd of this run holds a claude");
    assert_eq!(rigs_claude(&OsString::new(), &s), Err(None));
}

#[test]
fn a_link_in_the_rigs_bin_to_a_claude_outside_is_not_the_rigs() {
    let s = scratch("link");
    let out = plant(&s, "out");
    std::fs::create_dir_all(s.join("rig/bin")).unwrap();
    std::os::unix::fs::symlink(out.join("claude"), s.join("rig/bin/claude")).unwrap();
    let got = rigs_claude(&path(&[&s.join("rig/bin")]), &s.join("rig"));
    assert_eq!(got, Err(Some(s.join("rig/bin/claude"))));
}

#[test]
fn an_entry_that_climbs_out_of_the_rig_is_not_the_rigs() {
    let s = scratch("dotdot");
    plant(&s, "out");
    std::fs::create_dir_all(s.join("rig")).unwrap();
    let entry = s.join("rig/../out");
    assert_eq!(rigs_claude(&path(&[&entry]), &s.join("rig")), Err(Some(entry.join("claude"))));
}

/// Mode 611: the others' bit is set and the owner's is not, so this user's
/// spawn passes over it. A run as root would start it.
#[test]
fn a_claude_its_owner_may_not_execute_is_walked_past() {
    use std::os::unix::fs::PermissionsExt;
    let s = scratch("owner");
    let (rig, out) = (plant(&s, "rig/bin"), plant(&s, "out"));
    std::fs::set_permissions(rig.join("claude"), std::fs::Permissions::from_mode(0o611)).unwrap();
    assert_eq!(rigs_claude(&path(&[&rig, &out]), &s.join("rig")), Err(Some(out.join("claude"))));
}

#[test]
#[should_panic(expected = "dir must be absolute")]
fn a_relative_dir_is_refused_by_name() {
    let _ = rigs_claude(&OsString::from("/nowhere"), Path::new("rig"));
}

#[test]
fn rig_path_makes_the_bin_writes_the_stub_and_puts_the_bin_first() {
    use std::os::unix::fs::PermissionsExt;
    let s = scratch("rigpath");
    let bin = s.join("rig/bin");
    let got = rig_path(&bin);
    let old = std::env::var("PATH").unwrap();
    assert_eq!(got, format!("{}:{old}", bin.display()));
    let body = "#!/bin/sh\necho x >> \"$0.started\"\nexit 127\n";
    assert_eq!(std::fs::read_to_string(bin.join("claude")).unwrap(), body);
    assert_eq!(std::fs::metadata(bin.join("claude")).unwrap().permissions().mode() & 0o777, 0o755);
    assert_eq!(rigs_claude(&OsString::from(&got), &s.join("rig")), Ok(bin.join("claude")));
}

#[test]
fn the_stub_prints_nothing_and_exits_127() {
    let s = scratch("stub");
    let bin = s.join("bin");
    rig_path(&bin);
    // by its full path: this row starts the stub it has just written and nothing else
    let out = std::process::Command::new(bin.join("claude"))
        .args(["agents", "--json"])
        .output()
        .unwrap();
    assert_eq!((out.status.code(), out.stdout.len(), out.stderr.len()), (Some(127), 0, 0));
}

#[test]
fn the_stub_counts_each_start_in_a_file_beside_itself() {
    let s = scratch("count");
    let bin = s.join("bin");
    rig_path(&bin);
    assert!(!bin.join("claude.started").exists(), "counted before any start");
    for _ in 0..2 {
        // by its full path, as above
        std::process::Command::new(bin.join("claude")).status().unwrap();
    }
    assert_eq!(std::fs::read_to_string(bin.join("claude.started")).unwrap(), "x\nx\n");
}

#[test]
fn rig_path_keeps_a_claude_the_rig_wrote_itself() {
    let s = scratch("own");
    let bin = plant(&s, "bin");
    rig_path(&bin);
    assert_eq!(std::fs::read_to_string(bin.join("claude")).unwrap(), "#!/bin/sh\nexit 1\n");
}

#[test]
#[should_panic(expected = "bin must be absolute")]
fn rig_path_refuses_a_relative_bin_by_name() {
    rig_path(Path::new("bin"));
}

#[test]
#[should_panic(expected = "bin must hold no ':'")]
fn rig_path_refuses_a_bin_that_would_split_the_path() {
    rig_path(&std::env::temp_dir().join("cockpit-rigclaude-a:b"));
}

/// The child half of the row below: no PATH at all, and a bin that holds a
/// `claude` already, so the child writes nothing and starts nothing.
#[test]
fn unset_path_child() {
    let Some(bin) = std::env::var_os("RIGCLAUDE_UNSET_PATH") else {
        return;
    };
    assert_eq!(std::env::var_os("PATH"), None);
    let bin = PathBuf::from(bin);
    assert_eq!(rig_path(&bin), format!("{}:/bin:/usr/bin", bin.display()));
    eprintln!("the-default-search-was-kept");
}

#[test]
fn an_unset_path_keeps_the_default_search_after_the_bin() {
    let s = scratch("unset");
    let bin = plant(&s, "bin");
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "unset_path_child", "--nocapture"])
        .env("RIGCLAUDE_UNSET_PATH", &bin)
        .env_remove("PATH")
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success() && said.lines().any(|l| l == "the-default-search-was-kept"),
        "{said}"
    );
}

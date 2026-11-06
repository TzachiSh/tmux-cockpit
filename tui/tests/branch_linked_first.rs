//! #560: only a first record with no `.git` inside is a git dir to drop (#537). A
//! linked tree's `.git` is a file, so a linked tree listed first keeps its record.
//! A git shim prints git's own `-z` list with the linked tree's record moved first.
mod fake;
#[allow(dead_code)] // its -z refusing shim is not used: this file writes its own
mod oldgit;
use cockpit_tui::data::branches;
use std::process::Command;

#[test]
fn a_linked_tree_listed_first_keeps_its_record() {
    let (root, repo) = oldgit::repo("linkfirst");
    let l1 = root.join("l1");
    oldgit::add(&repo, "l1", &l1);
    let args = ["-C", repo.to_str().unwrap(), "worktree", "list", "--porcelain", "-z"];
    let z = Command::new("git").args(args).output().unwrap().stdout;
    let z = String::from_utf8(z).unwrap();
    let mut recs: Vec<&str> = z.split("\0\0").filter(|r| !r.is_empty()).collect();
    let first = format!("worktree {}\0", l1.display());
    recs.sort_by_key(|r| !r.starts_with(&first));
    assert!(recs[0].starts_with(&first), "the shim lists l1 first: {recs:?}");
    let list = root.join("list.z");
    std::fs::write(&list, recs.join("\0\0") + "\0\0").unwrap();
    let real = Command::new("sh").args(["-c", "command -v git"]).output().unwrap().stdout;
    let real = String::from_utf8(real).unwrap();
    let script = format!(
        "#!/bin/sh\ncase \" $* \" in *\" worktree list \"*\" -z \"*) exec cat '{}' ;; esac\n\
         exec {} \"$@\"\n",
        list.display(),
        real.trim()
    );
    let dir = root.join("shim");
    std::fs::create_dir_all(&dir).unwrap();
    fake::write_exec(&dir.join("git"), &script);
    let path = format!("{}:{}", dir.display(), std::env::var("PATH").unwrap());
    // SAFETY: this file holds one test, so no other thread reads the environment.
    unsafe { std::env::set_var("PATH", path) };
    let bs = branches::list(repo.to_str().unwrap());
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(oldgit::held(&bs, "l1"), Some(l1.display().to_string()), "l1 is kept");
}

//! #532: without `-z`, `locked` and `prunable` are attributes only alone or before a
//! space and a reason, as git prints them; `<path><NL>lockedx` cuts a path.
mod fake;
mod oldgit;
use cockpit_tui::data::branches;

#[test]
fn without_z_a_line_that_only_starts_with_locked_or_prunable_cuts_the_record() {
    let (root, repo) = oldgit::repo("lock");
    let w = root.join("w");
    oldgit::add(&repo, "w", &w);
    oldgit::git(&repo, &["worktree", "lock", "--reason", "on usb", w.to_str().unwrap()]);
    oldgit::add(&repo, "cutl", &root.join("w\nlockedx"));
    oldgit::add(&repo, "cutp", &root.join("w\nprunablex"));
    oldgit::shim(&root);
    let bs = branches::list(repo.to_str().unwrap());
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(oldgit::held(&bs, "w"), Some(w.display().to_string()), "locked <reason>");
    assert_eq!(oldgit::held(&bs, "cutl"), None, "lockedx is no label");
    assert_eq!(oldgit::held(&bs, "cutp"), None, "prunablex is no label");
}

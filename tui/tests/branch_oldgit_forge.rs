//! #348: without `-z`, a worktree named `inj<NL>worktree <dir>` prints a line that
//! reads as a worktree at `<dir>`. A record is a worktree only with a `.git` in it.
mod oldgit;
use cockpit_tui::data::branches;

#[test]
fn without_z_a_forged_worktree_line_is_dropped_and_the_main_checkout_kept() {
    let (root, repo) = oldgit::repo("forge");
    let plain = root.join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let wt = root.join("wt");
    oldgit::add(&repo, "linked", &wt);
    oldgit::add(&repo, "odd", &root.join(format!("inj\nworktree {}", plain.display())));
    oldgit::shim(&root);
    let bs = branches::list(wt.to_str().unwrap());
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(oldgit::held(&bs, "odd"), None, "a plain dir named by a forged line");
    assert_eq!(oldgit::held(&bs, "main"), Some(repo.display().to_string()), ".git dir");
}

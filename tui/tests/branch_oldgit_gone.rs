//! #348: without `-z`, the `.git` check drops a forged record, not a worktree
//! whose directory was deleted: that one stays listed, so goto can say it is gone.
mod oldgit;
use cockpit_tui::data::branches;

#[test]
fn without_z_a_worktree_whose_dir_is_gone_stays_listed() {
    let (root, repo) = oldgit::repo("gone");
    let gone = root.join("gone");
    oldgit::add(&repo, "gone", &gone);
    std::fs::remove_dir_all(&gone).unwrap();
    oldgit::shim(&root);
    let bs = branches::list(repo.to_str().unwrap());
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(oldgit::held(&bs, "gone"), Some(gone.display().to_string()));
}

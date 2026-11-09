//! #696: a gone tree's absolute `gitdir` holding a symlink before `..`. git lists it as
//! written, `<root>/lnk/../lw`; resolved as text that is `<root>/lw`, not where `lnk/..`
//! leads through the link. A forged line naming that place names no entry, so it is dropped.
mod fake;
mod oldgit;
use cockpit_tui::data::branches;

#[test]
fn without_z_a_symlink_before_dotdot_in_a_gone_gitdir_is_resolved_as_text() {
    let (root, repo) = oldgit::repo("gonelnk");
    std::fs::create_dir_all(root.join("sub/deep")).unwrap();
    std::os::unix::fs::symlink(root.join("sub/deep"), root.join("lnk")).unwrap();
    let lw = root.join("lw");
    oldgit::add(&repo, "lw", &lw);
    let listed = format!("{}/lnk/../lw", root.display());
    std::fs::write(repo.join(".git/worktrees/lw/gitdir"), format!("{listed}/.git\n")).unwrap();
    std::fs::remove_dir_all(&lw).unwrap();
    let led = root.join("sub/lw");
    oldgit::add(&repo, "fs", &root.join(format!("fs\nworktree {}", led.display())));
    oldgit::shim(&root);
    let bs = branches::list(repo.to_str().unwrap());
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(oldgit::held(&bs, "lw"), Some(listed), "the gone tree as git lists it");
    assert_eq!(oldgit::held(&bs, "fs"), None, "a forged line naming where lnk/.. leads");
}

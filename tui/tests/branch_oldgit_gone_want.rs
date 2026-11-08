//! #584: git lists an absolute `gitdir` as written, so a gone tree whose `gitdir`
//! holds `x/..` is listed as `<root>/x/../want`. Without `-z` it stays listed only
//! when the wanted `<path>/.git` is resolved as text too, not just the `gitdir`.
mod fake;
mod oldgit;
use cockpit_tui::data::branches;

#[test]
fn without_z_a_gone_tree_whose_absolute_gitdir_holds_x_dotdot_stays_listed() {
    let (root, repo) = oldgit::repo("gonewant");
    let want = root.join("want");
    oldgit::add(&repo, "want", &want);
    let listed = format!("{}/x/../want", root.display());
    let gitdir = repo.join(".git/worktrees/want/gitdir");
    std::fs::write(gitdir, format!("{listed}/.git\n")).unwrap();
    std::fs::remove_dir_all(&want).unwrap();
    oldgit::shim(&root);
    let bs = branches::list(repo.to_str().unwrap());
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(oldgit::held(&bs, "want"), Some(listed));
}

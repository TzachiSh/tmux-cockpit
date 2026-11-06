//! #555: without `-z`, a deleted tree stays listed when its `gitdir` names it only
//! once `.` and `..` are resolved: `--relative-paths` writes `../../../../rel/.git`,
//! taken from the entry, and a hand-edited one may hold `.` or `x/..`, which git
//! resolves as well. Both are written here, so no git needs `--relative-paths`.
mod fake;
mod oldgit;
use cockpit_tui::data::branches;

#[test]
fn without_z_a_gone_tree_whose_gitdir_needs_dots_resolved_stays_listed() {
    let (root, repo) = oldgit::repo("gonerel");
    let (rel, dots) = (root.join("rel"), root.join("dots"));
    oldgit::add(&repo, "rel", &rel);
    oldgit::add(&repo, "dots", &dots);
    let entries = repo.join(".git/worktrees");
    std::fs::write(entries.join("rel/gitdir"), "../../../../rel/.git\n").unwrap();
    std::fs::write(entries.join("dots/gitdir"), ".././../../../x/../dots/.git\n").unwrap();
    std::fs::remove_dir_all(&rel).unwrap();
    std::fs::remove_dir_all(&dots).unwrap();
    oldgit::shim(&root);
    let bs = branches::list(repo.to_str().unwrap());
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(oldgit::held(&bs, "rel"), Some(rel.display().to_string()), "relative");
    assert_eq!(oldgit::held(&bs, "dots"), Some(dots.display().to_string()), ". and ..");
}

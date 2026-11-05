//! #532: git 2.7 to 2.12 print `rev-parse --git-common-dir` as the subdirectory
//! prefix plus the path from the top. Read from a pane's subdirectory, that names no
//! dir, so the common dir is asked for from the top.
mod oldgit;
use cockpit_tui::data::branches;

#[test]
fn without_z_on_git_2_12_a_subdirectory_still_finds_the_trees() {
    let (root, repo) = oldgit::repo("subdir");
    let deep = repo.join("sub/deep");
    std::fs::create_dir_all(&deep).unwrap();
    let w = root.join("w");
    oldgit::add(&repo, "w", &w);
    oldgit::shim_212(&root);
    let bs = branches::list(deep.to_str().unwrap());
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(oldgit::held(&bs, "w"), Some(w.display().to_string()));
}

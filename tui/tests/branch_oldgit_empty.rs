//! #532: without `-z`, a tree at `<dir><NL>worktree ` prints a `worktree ` line with
//! an empty path. It names nothing, so it is dropped, as tmux-branch drops it.
mod oldgit;
use cockpit_tui::data::branches;

#[test]
fn without_z_an_empty_worktree_path_is_dropped() {
    let (root, repo) = oldgit::repo("empty");
    oldgit::add(&repo, "emp", &root.join("e\nworktree "));
    oldgit::shim(&root);
    let bs = branches::list(repo.to_str().unwrap());
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(oldgit::held(&bs, "emp"), None);
}

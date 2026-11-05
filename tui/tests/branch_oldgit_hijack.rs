//! #532: without `-z`, a tree at `<x><NL>worktree <w>` prints a record that pairs a
//! real tree's path `<w>` with the cut tree's branch. `<w>`'s own HEAD names its
//! real branch, so the forged branch is not hung on `<w>`.
mod oldgit;
use cockpit_tui::data::branches;

#[test]
fn without_z_a_branch_is_kept_only_where_that_trees_head_names_it() {
    let (root, repo) = oldgit::repo("hijack");
    let lw = root.join("lw");
    oldgit::add(&repo, "lw", &lw);
    let x = root.join("x");
    oldgit::add(&repo, "x", &x);
    oldgit::add(&repo, "fx", &root.join(format!("fx\nworktree {}", lw.display())));
    oldgit::add(&repo, "fm", &root.join(format!("fm\nworktree {}", repo.display())));
    oldgit::shim(&root);
    let bs = branches::list(x.to_str().unwrap());
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(oldgit::held(&bs, "lw"), Some(lw.display().to_string()), "the real tree");
    assert_eq!(oldgit::held(&bs, "main"), Some(repo.display().to_string()), "main");
    assert_eq!(oldgit::held(&bs, "fx"), None, "a linked tree's path, another branch");
    assert_eq!(oldgit::held(&bs, "fm"), None, "the main checkout's path, another branch");
}

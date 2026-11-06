//! #537: without `-z`, a missing path is kept only when one of this repo's
//! `worktrees/*/gitdir` files names `<path>/.git`, the test git uses to call a tree
//! prunable. A forged line naming a missing path names no entry, so it is dropped.
mod fake;
mod oldgit;
use cockpit_tui::data::branches;

#[test]
fn without_z_a_missing_path_is_kept_only_where_an_entry_names_it() {
    let (root, repo) = oldgit::repo("fgone");
    let gone = root.join("gone");
    oldgit::add(&repo, "gone", &gone);
    std::fs::remove_dir_all(&gone).unwrap();
    for (b, d) in [("fn", "nothere"), ("fg", "gonex")] {
        oldgit::add(&repo, b, &root.join(format!("{b}\nworktree {}", root.join(d).display())));
    }
    oldgit::shim(&root);
    let bs = branches::list(repo.to_str().unwrap());
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(oldgit::held(&bs, "gone"), Some(gone.display().to_string()), "a real deleted tree");
    assert_eq!(oldgit::held(&bs, "fn"), None, "a forged missing path");
    assert_eq!(oldgit::held(&bs, "fg"), None, "a forged path one letter off a gone tree");
}

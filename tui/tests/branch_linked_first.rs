//! #560: a linked tree's `.git` is a `gitdir:` file, so a linked tree listed first
//! is a tree and keeps its record; only a git dir listed as the main record is
//! dropped (#537). A git shim lists the linked tree's record first.
mod fake;
mod oldgit;
use cockpit_tui::data::branches;

#[test]
fn a_linked_tree_listed_first_keeps_its_record() {
    let (root, repo) = oldgit::repo("linkfirst");
    let l1 = root.join("l1");
    oldgit::add(&repo, "l1", &l1);
    oldgit::shim_first(&root, &repo, &l1);
    let bs = branches::list(repo.to_str().unwrap());
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(oldgit::held(&bs, "l1"), Some(l1.display().to_string()), "l1 is kept");
}

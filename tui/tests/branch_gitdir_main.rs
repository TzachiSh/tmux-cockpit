//! #537: when the main checkout's `.git` is a file (`--separate-git-dir`, or a
//! submodule), git prints the git dir as the main record's path. That dir is no
//! tree to open, so the record is dropped, with `-z` as without it. Only that one
//! (#555): the other linked tree keeps its ⑂.
mod fake;
mod oldgit;
use cockpit_tui::data::branches;
use std::path::PathBuf;

/// A repo whose git dir is `store.git`, checked out in `wk`, with linked trees l1 and
/// l2 on branches of those names. Returns the root to remove, l1 and l2.
fn sepgit(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let (root, _) = oldgit::repo(tag);
    let (wk, l1, l2) = (root.join("wk"), root.join("l1"), root.join("l2"));
    let store = root.join("store.git");
    let (s, w) = (store.to_str().unwrap(), wk.to_str().unwrap());
    oldgit::git(&root, &["init", "-q", "-b", "main", "--separate-git-dir", s, w]);
    oldgit::git(&wk, &["commit", "-q", "--allow-empty", "-m", "i"]);
    oldgit::add(&wk, "l1", &l1);
    oldgit::add(&wk, "l2", &l2);
    (root, l1, l2)
}

/// A repo with submodule sm, whose git dir is `.git/modules/sm`, and linked trees sl1
/// and sl2 of sm on branches of those names. Returns the root to remove, sl1 and sl2.
fn subgit(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let (root, repo) = oldgit::repo(tag);
    let (sm, sl1, sl2) = (repo.join("sm"), root.join("sl1"), root.join("sl2"));
    let url = root.join("src");
    oldgit::git(&root, &["init", "-q", "-b", "main", url.to_str().unwrap()]);
    oldgit::git(&url, &["commit", "-q", "--allow-empty", "-m", "s"]);
    let u = url.to_str().unwrap();
    oldgit::git(&repo, &["-c", "protocol.file.allow=always", "submodule", "add", "-q", u, "sm"]);
    oldgit::add(&sm, "sl1", &sl1);
    oldgit::add(&sm, "sl2", &sl2);
    (root, sl1, sl2)
}

#[test]
fn a_main_record_that_names_a_separate_git_dir_is_dropped() {
    let (root, l1, _) = sepgit("sepgit");
    let bs = branches::list(l1.to_str().unwrap());
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(oldgit::held(&bs, "main"), None, "the git dir is not a tree");
}

#[test]
fn a_linked_tree_beside_a_separate_git_dir_keeps_its_fork() {
    let (root, l1, l2) = sepgit("sepgit2");
    let bs = branches::list(l1.to_str().unwrap());
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(oldgit::held(&bs, "l2"), Some(l2.display().to_string()));
}

#[test]
fn a_main_record_that_names_a_submodules_git_dir_is_dropped() {
    let (root, sl1, _) = subgit("subgit");
    let bs = branches::list(sl1.to_str().unwrap());
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(oldgit::held(&bs, "main"), None, "modules/sm is not a tree");
}

#[test]
fn a_linked_tree_beside_a_submodules_git_dir_keeps_its_fork() {
    let (root, sl1, sl2) = subgit("subgit2");
    let bs = branches::list(sl1.to_str().unwrap());
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(oldgit::held(&bs, "sl2"), Some(sl2.display().to_string()));
}

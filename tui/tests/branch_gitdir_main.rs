//! #537: when the main checkout's `.git` is a file (`--separate-git-dir`, or a
//! submodule), git prints the git dir as the main record's path. That dir is no
//! tree to open, so the record is dropped, with `-z` as without it.
mod fake;
#[allow(dead_code)] // no shim: these run on the installed git, which takes -z
mod oldgit;
use cockpit_tui::data::branches;

#[test]
fn a_main_record_that_names_a_separate_git_dir_is_dropped() {
    let (root, _) = oldgit::repo("sepgit");
    let (wk, l1) = (root.join("wk"), root.join("l1"));
    let store = root.join("store.git");
    let (s, w) = (store.to_str().unwrap(), wk.to_str().unwrap());
    oldgit::git(&root, &["init", "-q", "-b", "main", "--separate-git-dir", s, w]);
    oldgit::git(&wk, &["commit", "-q", "--allow-empty", "-m", "i"]);
    oldgit::add(&wk, "l1", &l1);
    let bs = branches::list(l1.to_str().unwrap());
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(oldgit::held(&bs, "main"), None, "the git dir is not a tree");
}

#[test]
fn a_main_record_that_names_a_submodules_git_dir_is_dropped() {
    let (root, repo) = oldgit::repo("subgit");
    let (sm, sl) = (repo.join("sm"), root.join("sl"));
    let url = root.join("src");
    oldgit::git(&root, &["init", "-q", "-b", "main", url.to_str().unwrap()]);
    oldgit::git(&url, &["commit", "-q", "--allow-empty", "-m", "s"]);
    let u = url.to_str().unwrap();
    oldgit::git(&repo, &["-c", "protocol.file.allow=always", "submodule", "add", "-q", u, "sm"]);
    oldgit::add(&sm, "sl", &sl);
    let bs = branches::list(sl.to_str().unwrap());
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(oldgit::held(&bs, "main"), None, "modules/sm is not a tree");
}

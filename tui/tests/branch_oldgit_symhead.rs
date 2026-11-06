//! #537: `core.preferSymlinkRefs` writes HEAD as a symlink to `refs/heads/<b>`.
//! Without `-z`, the HEAD check reads that link, so such a tree keeps its branch.
mod fake;
mod oldgit;
use cockpit_tui::data::branches;

#[test]
fn without_z_a_tree_whose_head_is_a_symlink_keeps_its_branch() {
    let (root, repo) = oldgit::repo("symhead");
    let (sy, x) = (root.join("sy"), root.join("x"));
    oldgit::add(&repo, "sy", &sy);
    oldgit::add(&repo, "x", &x);
    for d in [&sy, &repo] {
        let b = if d == &sy { "refs/heads/sy" } else { "refs/heads/main" };
        oldgit::git(d, &["-c", "core.preferSymlinkRefs=true", "symbolic-ref", "HEAD", b]);
    }
    oldgit::shim(&root);
    let bs = branches::list(x.to_str().unwrap());
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(oldgit::held(&bs, "sy"), Some(sy.display().to_string()), "a linked tree");
    assert_eq!(oldgit::held(&bs, "main"), Some(repo.display().to_string()), "the main checkout");
}

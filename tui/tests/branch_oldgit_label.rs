//! #348: without `-z`, `bare` and `detached` are attributes only as a whole line,
//! as git prints them; `<path><NL>barex` cuts a path, it labels nothing.
mod fake;
mod oldgit;
use cockpit_tui::data::branches;

#[test]
fn without_z_a_line_that_only_starts_with_bare_cuts_the_record() {
    let (root, repo) = oldgit::repo("label");
    let w = root.join("w");
    oldgit::add(&repo, "w", &w);
    oldgit::add(&repo, "cutb", &root.join("w\nbarex"));
    oldgit::add(&repo, "cutd", &root.join("w\ndetachedx"));
    oldgit::shim(&root);
    let bs = branches::list(repo.to_str().unwrap());
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(oldgit::held(&bs, "w"), Some(w.display().to_string()), "the real tree");
    assert_eq!(oldgit::held(&bs, "cutb"), None, "not w, which only shares the cut prefix");
    assert_eq!(oldgit::held(&bs, "cutd"), None, "detachedx is no label either");
}

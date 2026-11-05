//! #532: without `-z`, a forged `worktree <dir>` line may name another repo's
//! checkout, which has a `.git` of its own. A record is kept only as this repo's:
//! a `.git` dir that is its common dir, or a `.git` file whose gitdir sits in that
//! dir's `worktrees/` and links back to the same file.
mod oldgit;
use cockpit_tui::data::branches;

#[test]
fn without_z_only_this_repos_trees_are_kept() {
    let (root, repo) = oldgit::repo("other");
    let other = root.join("other");
    std::fs::create_dir_all(&other).unwrap();
    oldgit::git(&other, &["init", "-q", "-b", "main", "."]);
    oldgit::git(&other, &["commit", "-q", "--allow-empty", "-m", "i"]);
    let olink = root.join("olink");
    oldgit::add(&other, "ol", &olink);
    let ours = root.join("our tree");
    oldgit::add(&repo, "ours", &ours);
    let plant = root.join("plant");
    std::fs::create_dir_all(&plant).unwrap();
    std::fs::copy(ours.join(".git"), plant.join(".git")).unwrap();
    for (b, d) in [("fo", &other), ("fl", &olink), ("fp", &plant)] {
        oldgit::add(&repo, b, &root.join(format!("{b}\nworktree {}", d.display())));
    }
    oldgit::shim(&root);
    let bs = branches::list(repo.to_str().unwrap());
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(oldgit::held(&bs, "ours"), Some(ours.display().to_string()), "ours");
    assert_eq!(oldgit::held(&bs, "fo"), None, "another repo's main checkout");
    assert_eq!(oldgit::held(&bs, "fl"), None, "another repo's linked tree");
    assert_eq!(oldgit::held(&bs, "fp"), None, "a copied .git file that does not link back");
}

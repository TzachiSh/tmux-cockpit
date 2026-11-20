//! #1352: the `<repo>` block's path probe answers only for paths under the repo's
//! root, and the block names its checkout kind. Each test makes its own temp folders.
use cockpit_tui::panel::capture::repo_state_within;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

/// A temp folder, removed on drop.
struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("cockpit-repo-root-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d.canonicalize().unwrap())
    }

    fn at(&self, name: &str) -> String {
        self.0.join(name).display().to_string()
    }

    fn write(&self, name: &str) {
        let p = self.0.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "x\n").unwrap();
    }

    fn git(&self, args: &[&str]) {
        let o = Command::new("git")
            .arg("-C")
            .arg(&self.0)
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    }

    /// A repo on `main` with `src/a.rs` committed.
    fn repo(tag: &str) -> Dir {
        let r = Dir::new(tag);
        r.git(&["init", "-q", "-b", "main"]);
        r.write("src/a.rs");
        r.git(&["add", "-A"]);
        r.git(&["commit", "-q", "-m", "first"]);
        r
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The product's 300 ms budget leaves a part out on a loaded box (#596).
fn block(dir: &str, screen: &str) -> String {
    repo_state_within(dir, screen, Duration::from_secs(10))
}

/// The `paths on screen:` lines, trimmed; empty when the block has none.
fn probed(block: &str) -> Vec<String> {
    let mut it = block.lines().skip_while(|l| *l != "paths on screen:").skip(1);
    std::iter::from_fn(|| it.next()?.strip_prefix("  ").map(|l| l.trim().to_string())).collect()
}

#[test]
fn absolute_paths_outside_the_root_get_no_answer() {
    let (r, out) = (Dir::repo("abs"), Dir::new("abs-out"));
    out.write("secret.txt");
    let inside = r.at("src/a.rs");
    let screen = format!("@{inside} @{} @{}", out.at("secret.txt"), out.at("gone.txt"));
    assert_eq!(probed(&block(&r.at(""), &screen)), [format!("exists: @{inside}")]);
}

#[test]
fn a_symlink_out_of_the_root_gets_no_answer() {
    let (r, out) = (Dir::repo("link"), Dir::new("link-out"));
    out.write("secret.txt");
    std::os::unix::fs::symlink(&out.0, r.0.join("link")).unwrap();
    let got = probed(&block(&r.at(""), "link/secret.txt ./link/gone.md src/a.rs"));
    assert_eq!(got, ["exists: src/a.rs"]);
}

#[test]
fn a_home_path_outside_the_root_gets_no_answer() {
    let r = Dir::repo("home");
    let home = PathBuf::from(std::env::var("HOME").unwrap());
    assert!(!home.starts_with(&r.0), "the repo must not hold HOME");
    let screen = format!("~/.cockpit-no-such-{}/x.md ./gone.md", std::process::id());
    assert_eq!(probed(&block(&r.at(""), &screen)), ["missing: ./gone.md"]);
}

/// The root is compared resolved: a pane in a symlink to the repo still gets answers.
#[test]
fn a_root_reached_through_a_symlink_still_answers() {
    let (r, links) = (Dir::repo("alias"), Dir::new("alias-links"));
    std::os::unix::fs::symlink(&r.0, links.0.join("repo")).unwrap();
    let alias = links.at("repo");
    let got = probed(&block(&alias, &format!("@{alias}/src/a.rs ./gone.md")));
    assert_eq!(got, [format!("exists: @{alias}/src/a.rs"), "missing: ./gone.md".to_string()]);
}

/// Rows: the repo's own checkout; a `git worktree add` of it. The line follows `root:`.
#[test]
fn the_block_names_its_checkout_kind() {
    let r = Dir::repo("kind");
    let wt = Dir::new("kind-wt");
    std::fs::remove_dir(&wt.0).unwrap();
    r.git(&["worktree", "add", "-q", "-b", "lane", &wt.at("")]);
    for (name, dir, want) in [("main checkout", r.at(""), "main"), ("linked", wt.at(""), "linked worktree")] {
        let got = block(&dir, "");
        let mut lines = got.lines().skip_while(|l| !l.starts_with("root: ")).skip(1);
        assert_eq!(lines.next(), Some(format!("checkout: {want}").as_str()), "{name}:\n{got}");
    }
}

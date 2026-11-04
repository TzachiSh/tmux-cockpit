//! Git's global excludes (#363). Its own test binary: it sets HOME and
//! XDG_CONFIG_HOME for the whole process, which no other test may share.
use cockpit_tui::files;

/// Removes the fixture when the test ends, pass or fail.
struct Gone(std::path::PathBuf);

impl Drop for Gone {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn git_global_excludes_hide_and_alt_i_would_list_them() {
    let home = std::env::temp_dir().join(format!("cockpit-files-global-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let _gone = Gone(home.clone());
    let repo = home.join("repo");
    std::fs::create_dir_all(home.join("xdg/git")).unwrap();
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(home.join("xdg/git/ignore"), "*.tmp\n").unwrap();
    let ok = std::process::Command::new("git")
        .args(["init", "-q"])
        .arg(&repo)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    std::fs::write(repo.join("g.tmp"), b"x").unwrap();
    std::fs::write(repo.join("keep.rs"), b"x").unwrap();
    // SAFETY: the only test in this binary, so nothing else reads the environment meanwhile.
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_CONFIG_HOME", home.join("xdg"));
    }
    let plain = files::list_dir(&repo, false, false).unwrap();
    assert_eq!(plain.iter().map(|e| e.name.clone()).collect::<Vec<_>>(), [b"keep.rs".to_vec()]);
    let all = files::list_dir(&repo, false, true).unwrap();
    assert_eq!(
        all.iter().filter(|e| e.ignored).map(|e| e.name.clone()).collect::<Vec<_>>(),
        [b"g.tmp".to_vec()]
    );
}

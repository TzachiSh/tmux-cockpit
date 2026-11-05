//! A fetch is one `tmux list-panes`: `@cockpit_dir` rides on it, so the git
//! facts no longer fork a second `list-panes -a` per tick. Its own test binary,
//! as `forks.rs` is: `list_panes` keeps thread-local caches and reads the env.
mod fake;

use cockpit_tui::data::list_panes;
use fake::write_exec;

#[test]
fn one_fetch_is_one_tmux_call_and_cockpit_dir_comes_off_it() {
    let dir = std::env::temp_dir().join(format!("cockpit-tui-forks-dirs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    std::fs::create_dir_all(dir.join("wt")).unwrap();
    let d = dir.display().to_string();
    // %1 names a tree that exists, %2 one that does not, %3 an older 13-field row
    let row = |id: &str, cwd: &str, opt: Option<&str>| {
        let mut f = vec![id, "w:0", "win", "claude", "running", "-", "-", "s1", cwd, "100", "-", "-", "@1"];
        f.extend(opt);
        f.join("\u{1f}")
    };
    let wt = format!("{d}/wt");
    let rows = [
        row("%1", "/main/one", Some(&wt)),
        row("%2", "/main/two", Some("/no/such/tree")),
        row("%3", "/main/three", None),
    ]
    .join("\n");
    std::fs::write(dir.join("rows"), rows + "\n").unwrap();
    write_exec(
        &dir.join("bin/tmux"),
        &format!("#!/bin/sh\necho \"$1\" >> {d}/tmux\ncase \"$1\" in list-panes) cat {d}/rows ;; esac\n"),
    );
    // records the tree each probe ran in: `git -C <dir> ...`
    write_exec(&dir.join("bin/git"), &format!("#!/bin/sh\necho \"$2\" >> {d}/git\nexit 1\n"));
    write_exec(&dir.join("bin/ps"), "#!/bin/sh\n");
    // SAFETY: this test binary holds exactly one test, so nothing runs beside it.
    unsafe {
        std::env::set_var("PATH", format!("{d}/bin:{}", std::env::var("PATH").unwrap_or_default()));
        std::env::set_var("COCKPIT_TMUX", format!("{d}/bin/tmux"));
        std::env::set_var("HOME", &d);
        std::env::set_var("COCKPIT_READONLY", "1");
        // #505: no proc under it, so the walk asks the fake ps, not the live pid of the same number
        std::env::set_var("COCKPIT_PROC_DIR", format!("{d}/proc"));
    }

    assert_eq!(list_panes().unwrap().len(), 3);
    assert_eq!(list_panes().unwrap().len(), 3);

    // #301 reads the agent list for every row, so the first fetch also makes
    // the thread's one `show-option` for it; every fetch after is one call
    let calls = std::fs::read_to_string(dir.join("tmux")).unwrap_or_default();
    assert_eq!(calls, "list-panes\nshow-option\nlist-panes\n", "one tmux call per fetch");
    let mut trees: Vec<String> = std::fs::read_to_string(dir.join("git"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    trees.sort();
    let mut want = vec![wt, "/main/three".to_string(), "/main/two".to_string()];
    want.sort();
    assert_eq!(trees, want, "@cockpit_dir when it is a directory, else the cwd");
    let _ = std::fs::remove_dir_all(&dir);
}

//! #1343: `data::workdir::choose`, the one rule for the tree a pane works in, and
//! where it came from. Real folders, since a candidate counts only when it names one.
use cockpit_tui::data::workdir::{ACTIVE_MAX_AGE, Candidates, RECENT_MAX, Recent, Source, choose};
use std::path::PathBuf;

/// Three real folders, removed on drop.
struct Dirs(PathBuf);

impl Dirs {
    fn new(tag: &str) -> Dirs {
        let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("choose-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for n in ["a", "b", "c", "t"] {
            std::fs::create_dir_all(d.join(n)).unwrap();
        }
        Dirs(d)
    }

    fn at(&self, n: &str) -> String {
        self.0.join(n).display().to_string()
    }
}

impl Drop for Dirs {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn recent(dir: &str, age: u64) -> Recent {
    Recent { dir: dir.into(), age }
}

#[test]
fn each_candidate_wins_in_its_turn() {
    let d = Dirs::new("order");
    let (a, b, t, gone) = (d.at("a"), d.at("b"), d.at("t"), d.at("gone"));
    let pane = d.at("c");
    let base = Candidates {
        pane_cwd: pane.clone(),
        ..Default::default()
    };
    // rows: what is set, then the folder and source that answer
    let rows = [
        (
            "declared beats hook_cwd",
            Candidates {
                declared: a.clone(),
                hook_cwd: b.clone(),
                ..base.clone()
            },
            &a,
            Source::Declared,
        ),
        // decision 1 on #1342: active-in stands for the folder only when nothing was declared
        (
            "declared beats a fresh active entry",
            Candidates {
                declared: a.clone(),
                active: vec![recent(&b, 0)],
                ..base.clone()
            },
            &a,
            Source::Declared,
        ),
        (
            "a fresh active entry beats the session",
            Candidates {
                active: vec![recent(&a, 0)],
                hook_cwd: b.clone(),
                ..base.clone()
            },
            &a,
            Source::ActiveIn,
        ),
        (
            "one a second short of stale still counts",
            Candidates {
                active: vec![recent(&a, ACTIVE_MAX_AGE - 1)],
                hook_cwd: b.clone(),
                ..base.clone()
            },
            &a,
            Source::ActiveIn,
        ),
        (
            "a stale active entry does not",
            Candidates {
                active: vec![recent(&a, ACTIVE_MAX_AGE)],
                hook_cwd: b.clone(),
                ..base.clone()
            },
            &b,
            Source::Session,
        ),
        (
            "only the newest active entry counts",
            Candidates {
                active: vec![recent(&a, 9), recent(&gone, 2)],
                hook_cwd: b.clone(),
                ..base.clone()
            },
            &b,
            Source::Session,
        ),
        (
            "a relative declared falls through",
            Candidates {
                declared: ".".into(),
                hook_cwd: b.clone(),
                ..base.clone()
            },
            &b,
            Source::Session,
        ),
        (
            "a missing declared falls through",
            Candidates {
                declared: gone.clone(),
                hook_cwd: b.clone(),
                ..base.clone()
            },
            &b,
            Source::Session,
        ),
        (
            "a missing hook_cwd falls to the transcript",
            Candidates {
                hook_cwd: gone.clone(),
                transcript_cwd: t.clone(),
                ..base.clone()
            },
            &t,
            Source::Session,
        ),
        (
            "nothing that holds is the pane's cwd",
            Candidates {
                declared: "rel".into(),
                transcript_cwd: gone.clone(),
                ..base.clone()
            },
            &pane,
            Source::PaneCwd,
        ),
    ];
    for (name, c, dir, source) in rows {
        let w = choose(&c);
        assert_eq!((w.dir.as_str(), w.source), (dir.as_str(), source), "{name}");
    }
}

#[test]
fn the_pane_cwd_answers_even_when_it_is_no_folder() {
    for cwd in ["rel", "", "/no/such/dir"] {
        let w = choose(&Candidates {
            pane_cwd: cwd.into(),
            ..Default::default()
        });
        assert_eq!((w.dir.as_str(), w.source, w.cwd.as_str()), (cwd, Source::PaneCwd, cwd), "{cwd:?}");
    }
}

#[test]
fn cwd_is_always_the_panes_whoever_wins() {
    let d = Dirs::new("cwd");
    let c = Candidates {
        declared: d.at("a"),
        pane_cwd: d.at("c"),
        ..Default::default()
    };
    assert_eq!(choose(&c).cwd, d.at("c"));
}

#[test]
fn recent_is_newest_first_and_capped() {
    let given = [
        recent("/w", 700),
        recent("/x", 30),
        recent("/y", 5),
        recent("/z", 400),
        recent("/v", 90),
    ];
    let w = choose(&Candidates {
        active: given.to_vec(),
        ..Default::default()
    });
    assert_eq!(RECENT_MAX, 3);
    assert_eq!(w.recent, vec![recent("/y", 5), recent("/x", 30), recent("/v", 90)]);
}

#[test]
fn the_sources_read_as_the_freeze_names_them() {
    let s = [Source::Declared, Source::ActiveIn, Source::Session, Source::PaneCwd].map(Source::as_str);
    assert_eq!(s, ["declared", "active-in", "session", "pane-cwd"]);
}

#[test]
fn the_freeze_holds_its_two_numbers() {
    assert_eq!((ACTIVE_MAX_AGE, RECENT_MAX), (600, 3));
}

#[test]
fn a_dir_differs_from_the_hooks_cwd_only_when_set_by_hand() {
    // rows: @cockpit_dir, @cockpit_cwd -> declared, hook_cwd
    let rows = [
        ("set by hand", "/a", "/b", "/a", "/b"),
        ("written by the hooks", "/a", "/a", "", "/a"),
        ("no hook cwd", "/a", "", "/a", "/a"),
        ("no dir", "", "/b", "", "/b"),
        ("neither", "", "", "", ""),
    ];
    for (name, dir, cwd, declared, hook) in rows {
        let c = Candidates::from_options(dir, cwd);
        assert_eq!((c.declared.as_str(), c.hook_cwd.as_str()), (declared, hook), "{name}");
    }
}

/// #1377, moved from the `work_dirs` rows of tests/fleet.rs: of the two session cwds the
/// hook's ranks first, and a relative one never counts (#452), so both fall to the pane.
#[test]
fn the_session_cwds_rank_hook_first_and_a_relative_one_falls_through() {
    let d = Dirs::new("session");
    let (a, t, pane) = (d.at("a"), d.at("t"), d.at("c"));
    // rows: hook_cwd, transcript_cwd, then the folder and source that answer
    let rows = [
        ("hook beats transcript", a.as_str(), t.as_str(), a.as_str(), Source::Session),
        ("relative ones fall through", ".", "src", pane.as_str(), Source::PaneCwd),
    ];
    for (name, hook, said, dir, source) in rows {
        let w = choose(&Candidates {
            hook_cwd: hook.into(),
            transcript_cwd: said.into(),
            pane_cwd: pane.clone(),
            ..Default::default()
        });
        assert_eq!((w.dir.as_str(), w.source), (dir, source), "{name}");
    }
}

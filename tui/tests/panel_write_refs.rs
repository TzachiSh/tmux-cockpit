//! A Write carries the prompt basket (#365 child 5, PR 5): ^S with refs in the
//! basket sends them in a `<refs>` block, in the form the pane's agent reads
//! (owner's D4), a whole file marked and, while the write explores, a ref
//! outside the folder it may read marked as not read. The basket rides along
//! with a written result that names every ref, for cockpit-panel to clear once
//! the paste lands. No tmux: the basket is set on the App as main would read it.
use cockpit_tui::panel::action::sanitize;
use cockpit_tui::panel::app::{App, View};
use cockpit_tui::panel::capture::Capture;
use cockpit_tui::panel::envelope::{Flow, OutType, RefItem, build, build_refs, esc, ref_items};
use cockpit_tui::panel::request::Fail;
use cockpit_tui::refs::Ref;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::{Path, PathBuf};

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A temp dir holding `repo/a.rs`, `repo/b.md` and `other/c.rs`, canonical;
/// `repo/.git` marks the project an ask needs.
fn tree(tag: &str) -> Dir {
    let d = std::env::temp_dir().join(format!("cockpit-write-refs-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("repo/.git")).unwrap();
    for f in ["repo/a.rs", "repo/b.md", "other/c.rs"] {
        std::fs::create_dir_all(d.join(f).parent().unwrap()).unwrap();
        std::fs::write(d.join(f), "x\ny\nz\n").unwrap();
    }
    Dir(d.canonicalize().unwrap())
}

fn r(d: &Dir, f: &str, lines: Option<(u32, u32)>) -> Ref {
    Ref::new(d.0.join(f), lines, None).unwrap()
}

fn item(text: &str, whole: bool, outside: bool) -> RefItem {
    RefItem {
        text: text.into(),
        mention: text.starts_with('@'),
        whole,
        outside,
        denied: false,
    }
}

fn cap(path: &Path) -> Capture {
    Capture {
        name: "claude".into(),
        cmd: "claude".into(),
        path: path.display().to_string(),
        screen: "$ ".into(),
        ..Default::default()
    }
}

fn write() -> Flow {
    Flow::Write {
        draft: "fix it".into(),
        out: OutType::Auto,
    }
}

// ------------------------------------------------------------ envelope rows

#[test]
fn no_refs_is_no_block_and_the_envelope_as_before() {
    let d = tree("none");
    let c = cap(&d.0.join("repo"));
    assert_eq!(build_refs(&write(), &c, &[], 3, &[]).text, build(&write(), &c, &[], 3));
    assert!(ref_items(&[], &d.0, true, None).is_empty());
}

#[test]
fn the_block_lists_each_ref_before_the_request() {
    let d = tree("block");
    let items = [item("@a.rs#L1-2", false, false), item("@b.md", true, false)];
    let got = build_refs(&write(), &cap(&d.0.join("repo")), &[], 3, &items).text;
    let want = "<refs>\n<ref form=\"mention\">@a.rs#L1-2</ref>\n\
                <ref form=\"mention\" note=\"whole file\">@b.md</ref>\n</refs>\n\n<request>";
    assert!(got.contains(want), "{got}");
}

#[test]
fn claude_gets_mentions_and_another_agent_bare_refs() {
    let d = tree("forms");
    let refs = [r(&d, "repo/a.rs", Some((1, 2))), r(&d, "repo/b.md", None)];
    let base = d.0.join("repo");
    let text = |claude| {
        ref_items(&refs, &base, claude, None)
            .into_iter()
            .map(|i| i.text)
            .collect::<Vec<_>>()
    };
    assert_eq!(text(true), ["@a.rs#L1-2", "@b.md"]);
    assert_eq!(text(false), ["a.rs:1-2", "b.md"]);
}

/// #590: each ref says its form, so the model copies it rather than rewriting
/// a bare ref into Claude Code's @path#L mention.
#[test]
fn each_ref_says_its_form() {
    let d = tree("formattr");
    let refs = [r(&d, "repo/a.rs", Some((1, 2))), r(&d, "repo/b.md", None)];
    let base = d.0.join("repo");
    let block = |claude| build_refs(&write(), &cap(&base), &[], 3, &ref_items(&refs, &base, claude, None)).text;
    let bare = "<ref form=\"bare\">a.rs:1-2</ref>\n<ref form=\"bare\" note=\"whole file\">b.md</ref>";
    assert!(block(false).contains(bare), "{}", block(false));
    let mention = "<ref form=\"mention\">@a.rs#L1-2</ref>\n<ref form=\"mention\" note=\"whole file\">@b.md</ref>";
    assert!(block(true).contains(mention), "{}", block(true));
}

/// A claude pane falls back to a bare ref for a path a mention cannot carry
/// (a `#` or a `"`, basket::ref_line): its form is bare, never mention.
#[test]
fn a_claude_ref_that_falls_back_to_bare_says_bare() {
    let d = tree("formfall");
    std::fs::write(d.0.join("repo/a#b.rs"), "x\n").unwrap();
    let got = ref_items(&[r(&d, "repo/a#b.rs", Some((1, 1)))], &d.0.join("repo"), true, None);
    assert_eq!((got[0].text.as_str(), got[0].mention), ("a#b.rs:1", false));
}

/// Owner's call: a whole file is marked, never refused.
#[test]
fn a_whole_file_is_marked() {
    let d = tree("whole");
    let refs = [r(&d, "repo/a.rs", Some((2, 2))), r(&d, "repo/b.md", None)];
    let got = ref_items(&refs, &d.0.join("repo"), true, None);
    assert_eq!(got, [item("@a.rs#L2", false, false), item("@b.md", true, false)]);
}

/// Outside the read root is marked only when there is one: an exploring write.
#[test]
fn an_outside_ref_is_marked_only_with_a_root() {
    let d = tree("outside");
    let (base, refs) = (
        d.0.join("repo"),
        [r(&d, "repo/a.rs", Some((1, 1))), r(&d, "other/c.rs", Some((2, 3)))],
    );
    let marks = |root: Option<&Path>| ref_items(&refs, &base, false, root).iter().map(|i| i.outside).collect::<Vec<_>>();
    assert_eq!(marks(Some(&base)), [false, true]);
    assert_eq!(marks(None), [false, false]);
}

#[test]
fn an_outside_ref_says_it_was_not_read() {
    let d = tree("note");
    let items = [item("/x/c.rs:2-3", false, true), item("/x/d.rs", true, true)];
    let got = build_refs(&write(), &cap(&d.0.join("repo")), &[], 3, &items).text;
    assert!(
        got.contains("<ref form=\"bare\" note=\"outside the repo, not read\">/x/c.rs:2-3</ref>"),
        "{got}"
    );
    assert!(
        got.contains("<ref form=\"bare\" note=\"whole file; outside the repo, not read\">/x/d.rs</ref>"),
        "{got}"
    );
}

/// A root reached through a symlink is the folder it names: a ref inside that
/// folder is inside, so the check compares canonical paths on both sides.
#[test]
fn a_root_given_through_a_symlink_is_its_target() {
    let d = tree("rootlink");
    std::os::unix::fs::symlink(d.0.join("repo"), d.0.join("link")).unwrap();
    let got = ref_items(&[r(&d, "repo/a.rs", None)], &d.0.join("link"), false, Some(&d.0.join("link")));
    assert_eq!(got, [item("a.rs", true, false)]);
}

/// A ref is its file's canonical path: a link inside the root that leads out is
/// outside (the read would be refused), and a link outside that leads in is inside.
#[test]
fn a_ref_through_a_symlink_is_where_the_link_leads() {
    let d = tree("reflink");
    std::os::unix::fs::symlink(d.0.join("other/c.rs"), d.0.join("repo/out.rs")).unwrap();
    std::os::unix::fs::symlink(d.0.join("repo/a.rs"), d.0.join("other/in.rs")).unwrap();
    let (base, refs) = (d.0.join("repo"), [r(&d, "repo/out.rs", None), r(&d, "other/in.rs", None)]);
    let marks: Vec<bool> = ref_items(&refs, &base, false, Some(&base)).iter().map(|i| i.outside).collect();
    assert_eq!(marks, [true, false]);
}

/// The read-only profile denies secret files by rule (request.rs DENY): a ref
/// to one inside the root is noted as not read, as an outside one is.
#[test]
fn a_denied_file_in_the_root_is_noted_not_read() {
    let d = tree("denied");
    for f in ["repo/.env", "repo/.env.local", "repo/envy.rs", "other/.env"] {
        std::fs::write(d.0.join(f), "K=v\n").unwrap();
    }
    let base = d.0.join("repo");
    let refs = [
        r(&d, "repo/.env", None),
        r(&d, "repo/.env.local", Some((1, 1))),
        r(&d, "repo/envy.rs", None),
    ];
    let denied = |root: Option<&Path>| ref_items(&refs, &base, false, root).iter().map(|i| i.denied).collect::<Vec<_>>();
    assert_eq!(denied(Some(&base)), [true, true, false]);
    assert_eq!(denied(None), [false, false, false]);
    // outside the root it is noted outside only: one "not read" note a ref
    let out = ref_items(&[r(&d, "other/.env", None)], &base, false, Some(&base));
    assert_eq!((out[0].outside, out[0].denied), (true, false));
}

#[test]
fn a_denied_ref_says_it_was_not_read() {
    let d = tree("deniednote");
    let items = [RefItem {
        denied: true,
        ..item(".env", true, false)
    }];
    let got = build_refs(&write(), &cap(&d.0.join("repo")), &[], 3, &items).text;
    assert!(
        got.contains("<ref form=\"bare\" note=\"whole file; a secret the write may not read, not read\">.env</ref>"),
        "{got}"
    );
}

/// #586: a file under a folder named `.env` is refused by `Read(//**/.env)`
/// (probed), so a basket ref to it is noted as not read in the prompt a write
/// sends, end to end through the panel (review-534's test on #659).
#[test]
fn a_ref_under_a_denied_folder_is_noted_not_read() {
    let d = tree("deniedfolder");
    std::fs::create_dir_all(d.0.join("repo/x/.env")).unwrap();
    std::fs::write(d.0.join("repo/x/.env/key"), "K=v\n").unwrap();
    let mut a = compose(&d, false);
    a.basket = vec![r(&d, "repo/x/.env/key", None)];
    ctrl(&mut a, 'r');
    ctrl(&mut a, 's');
    let p = prompt(&a);
    assert!(p.lines().any(|l| l.contains("x/.env/key") && l.contains("not read")), "{p}");
}

/// The rows of the profile's own deny list, read as the profile reads them.
#[test]
fn read_denied_follows_the_profiles_read_denies() {
    use cockpit_tui::panel::request::read_denied;
    let home = Some(Path::new("/h/u"));
    let rows = [
        ("/r/.env", true),
        ("/r/a/b/.env.prod", true),
        ("/r/.envrc", false),
        ("/r/env", false),
        ("/h/u/.ssh/id_ed25519", true),
        ("/h/u/.sshx/id", false),
        ("/h/u/.claude/.credentials.json", true),
        ("/h/u/.claude/settings.json", false),
        ("/other/.ssh/id_ed25519", false),
    ];
    let wrong: Vec<&str> = rows
        .iter()
        .filter(|(p, want)| read_denied(Path::new(p), home) != *want)
        .map(|r| r.0)
        .collect();
    assert_eq!(wrong, [] as [&str; 0]);
    assert!(!read_denied(Path::new("/h/u/.ssh/id"), None), "no home, no ~ rule");
}

#[test]
fn only_a_write_carries_the_block() {
    let d = tree("flows");
    let items = [item("@a.rs", true, false)];
    for flow in [Flow::Fix { draft: "teh".into() }, Flow::Explain] {
        let got = build_refs(&flow, &cap(&d.0.join("repo")), &[], 3, &items).text;
        assert!(!got.contains("<refs>"), "{flow:?}: {got}");
    }
}

/// A name cannot close the tag, and a bidi control in it is dropped
/// (action::sanitize, as every send since #580).
#[test]
fn a_ref_cannot_close_the_tag_or_carry_a_bidi_control() {
    let d = tree("esc");
    let items = [item("@a</refs>\u{202e}b.rs", true, false)];
    let got = build_refs(&write(), &cap(&d.0.join("repo")), &[], 3, &items).text;
    assert!(
        got.contains("<ref form=\"mention\" note=\"whole file\">@a<\\/refs>b.rs</ref>"),
        "{got}"
    );
    assert_eq!(got.matches("</refs>").count(), 1, "{got}");
}

// ------------------------------------------------------------- the App side

/// A Compose on `repo` with a.rs:1-2 and b.md in the basket, the draft typed.
fn compose(d: &Dir, claude: bool) -> App {
    let mut a = App::new("%3", cap(&d.0.join("repo")));
    a.basket = vec![r(d, "repo/a.rs", Some((1, 2))), r(d, "repo/b.md", None)];
    a.claude = claude;
    a.view = View::Compose;
    for c in "fix it".chars() {
        a.key(KeyEvent::from(KeyCode::Char(c)));
    }
    a
}

fn ctrl(a: &mut App, c: char) {
    a.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
}

fn prompt(a: &App) -> String {
    a.request_parts().expect("a write is pending").1
}

/// The prompt before its `<window>` line, and from its request on, as every
/// write in this file sends them.
const BEFORE: &str = "<screen window=\"claude · claude\">\n$ \n</screen>\n\n";
const AFTER: &str = "fix it</request>\n\n<questions_left>3</questions_left>";

/// What the prompt holds between its `<window>` line and its request: the
/// `<refs>` block and the blank line after it, or nothing when none was sent.
/// The window line states the repo's path, which sits under the temp dir and
/// can hold any word a test looks for, so a test compares this part whole
/// (#910). The parts cut off are compared whole here, the window line with
/// the path as the envelope escapes it, so a block or a note written anywhere
/// else in the prompt fails too. `esc` leaves no `</` in a path, so the first
/// `</window>` is the line's own; the last `<request>` is the request's, the
/// draft holding none.
fn refs_sent(a: &App) -> String {
    let p = prompt(a);
    let cut = |what: &str| panic!("no {what}: {p}");
    let (before, rest) = p.split_once("<window>").unwrap_or_else(|| cut("window line"));
    let (window, rest) = rest.split_once("</window>\n\n").unwrap_or_else(|| cut("window end"));
    let (refs, after) = rest.rsplit_once("<request>").unwrap_or_else(|| cut("request"));
    let shown = format!("claude · claude · {}", esc(&a.cap.path));
    assert_eq!((before, window, after), (BEFORE, shown.as_str(), AFTER), "{p}");
    refs.to_string()
}

/// A ref outside the pane's folder as the block writes it: the absolute path,
/// cleaned and escaped as `refs_part` does, so a `</` in the temp dir's path
/// goes out as `<\/`.
fn abs(d: &Dir, f: &str, lines: &str) -> String {
    esc(&sanitize(&format!("{}:{lines}", d.0.join(f).display())))
}

/// The two refs `compose` puts in the basket, as a pane that is not claude gets them.
const INSIDE: &str = "<ref form=\"bare\">a.rs:1-2</ref>\n\
                      <ref form=\"bare\" note=\"whole file\">b.md</ref>";

#[test]
fn ctrl_s_with_a_basket_sends_the_refs() {
    let d = tree("ctrls");
    let mut a = compose(&d, true);
    ctrl(&mut a, 's');
    let refs = "<refs>\n<ref form=\"mention\">@a.rs#L1-2</ref>\n\
                <ref form=\"mention\" note=\"whole file\">@b.md</ref>\n</refs>";
    assert_eq!(refs_sent(&a), format!("{refs}\n\n"));
    a.basket.clear();
    assert_eq!(refs_sent(&a), "", "an empty basket sends no block");
}

/// #590: a write with a bare ref carries the example that keeps one bare and
/// keeps a rename small; a write with mentions only, or no refs, does not.
#[test]
fn a_bare_ref_brings_the_bare_ref_example() {
    let d = tree("bareex");
    let system = |claude: bool, basket: bool| {
        let mut a = compose(&d, claude);
        if !basket {
            a.basket.clear();
        }
        ctrl(&mut a, 's');
        a.request_parts().expect("a write is pending").0
    };
    let example = "rename the --dry flag to --dry-run";
    assert!(system(false, true).contains(example), "a bare ref");
    assert!(!system(true, true).contains(example), "mentions only");
    assert!(!system(false, false).contains(example), "no refs");
}

#[test]
fn an_exploring_write_marks_a_ref_outside_its_folder() {
    let d = tree("explore");
    let mut a = compose(&d, false);
    a.basket.push(r(&d, "other/c.rs", Some((2, 3))));
    ctrl(&mut a, 'r');
    assert!(a.explore, "{:?}", a.note);
    ctrl(&mut a, 's');
    let out = format!(
        "<ref form=\"bare\" note=\"outside the repo, not read\">{}</ref>",
        abs(&d, "other/c.rs", "2-3")
    );
    assert_eq!(refs_sent(&a), format!("<refs>\n{INSIDE}\n{out}\n</refs>\n\n"));
}

#[test]
fn a_write_without_explore_marks_nothing_outside() {
    let d = tree("noexplore");
    let mut a = compose(&d, false);
    a.basket.push(r(&d, "other/c.rs", Some((2, 3))));
    ctrl(&mut a, 's');
    // the third ref goes out with no note, compared whole: its path can hold `outside`
    let out = format!("<ref form=\"bare\">{}</ref>", abs(&d, "other/c.rs", "2-3"));
    assert_eq!(refs_sent(&a), format!("<refs>\n{INSIDE}\n{out}\n</refs>\n\n"));
}

const NAMED: &str = "<prompt>Fix it in @a.rs#L1-2, as @b.md says.</prompt>";

/// ^S, then the reply: a written result on screen.
fn written(d: &Dir, reply: &str) -> App {
    let mut a = compose(d, true);
    ctrl(&mut a, 's');
    a.reply(Ok(reply.into()));
    assert!(matches!(a.view, View::Result { .. }), "{:?}", a.view);
    a
}

/// The basket's text as main reads it: one absolute line per ref.
fn raw(a: &App) -> String {
    cockpit_tui::basket::body(&a.basket).unwrap()
}

#[test]
fn a_written_prompt_naming_the_refs_carries_the_basket_when_sent() {
    let d = tree("sent");
    let mut a = written(&d, NAMED);
    a.key(KeyEvent::from(KeyCode::Char('s')));
    a.key(KeyEvent::from(KeyCode::Char('y')));
    assert_eq!(a.action.as_ref().map(|x| x.verb), Some("send"));
    assert_eq!(a.sent_basket, Some(raw(&a)));
}

#[test]
fn a_written_prompt_naming_the_refs_carries_the_basket_when_inserted() {
    let d = tree("inserted");
    let mut a = written(&d, NAMED);
    a.key(KeyEvent::from(KeyCode::Char('i')));
    assert_eq!(a.action.as_ref().map(|x| x.verb), Some("paste"));
    assert_eq!(a.sent_basket, Some(raw(&a)));
}

/// The basket the panel read is what rides along, a gone file's line included
/// (review-534), so the clear still matches.
#[test]
fn the_basket_rides_along_as_it_was_read() {
    let d = tree("asread");
    let mut a = written(&d, NAMED);
    a.basket_raw = format!("{}\n{}/gone.rs", raw(&a), d.0.display());
    a.key(KeyEvent::from(KeyCode::Char('i')));
    assert_eq!(a.sent_basket.as_deref(), Some(a.basket_raw.as_str()));
}

/// A result that leaves a ref out did not carry it: the basket stays.
#[test]
fn a_written_prompt_missing_a_ref_leaves_the_basket() {
    let d = tree("missing");
    for reply in [
        "<prompt>Fix it in @a.rs#L1-2.</prompt>",
        "<prompt>Fix @a.rs#L1-20 per @b.md</prompt>",
    ] {
        let mut a = written(&d, reply);
        a.key(KeyEvent::from(KeyCode::Char('i')));
        assert!(a.action.is_some() && a.sent_basket.is_none(), "{reply}: {:?}", a.sent_basket);
    }
}

/// A ref is named only as a whole token, as given: a longer ref that holds it
/// (a range on a whole file, a longer range, a deeper path, another name) is
/// another ref, while quotes, brackets and a sentence's punctuation around it
/// are not. Each row is a basket and a written result: does it clear (every
/// ref named) or keep the basket? Rows from both reviews of #581.
#[test]
fn a_ref_is_named_only_as_a_whole_token() {
    use cockpit_tui::panel::app::unnamed;
    const CLEAR: bool = true;
    const KEEP: bool = false;
    let rows: &[(&str, &[&str], &str, bool)] = &[
        (
            "whole file rewritten to a range",
            &["@a.rs#L1-2", "@b.md"],
            "Fix @a.rs#L1-2 per @b.md#L5.",
            KEEP,
        ),
        (
            "whole file in a longer name",
            &["@a.rs#L1-2", "@b.md"],
            "Fix @a.rs#L1-2 per @b.md.bak.",
            KEEP,
        ),
        (
            "bare refs inside other names",
            &["a.rs:1-2", "b.md"],
            "Fix data.rs:1-2 per web.md.",
            KEEP,
        ),
        ("bare whole, a longer name", &["a.rs"], "Fix a.rs.bak now.", KEEP),
        ("bare whole, another name", &["a.rs"], "Fix data.rs now.", KEEP),
        ("bare whole, a deeper path", &["a.rs"], "Fix lib/a.rs now.", KEEP),
        ("bare whole, given a line", &["a.rs"], "Fix a.rs:5 now.", KEEP),
        ("mention whole, a longer name", &["@a.rs"], "Fix @a.rs.bak now.", KEEP),
        ("mention whole, given a line", &["@a.rs"], "Fix @a.rs#L5 now.", KEEP),
        ("bare range, more range after", &["a.rs:1-2"], "Fix a.rs:1-2-3 now.", KEEP),
        ("bare range, a longer range", &["a.rs:1-2"], "Fix a.rs:1-20 now.", KEEP),
        ("mention range, a longer range", &["@a.rs#L1-2"], "Fix @a.rs#L1-20 now.", KEEP),
        ("mention range, another range", &["@a.rs#L1-2"], "Fix @a.rs#L1-3 now.", KEEP),
        ("one line in a range", &["@a.rs#L1"], "Fix @a.rs#L1-2 now.", KEEP),
        ("a # name rewritten as a mention", &["a#b.rs:1-2"], "Fix @a#b.rs#L1-2 now.", KEEP),
        ("a colon then more range", &["a.rs:1-2"], "see a.rs:1-2:9", KEEP),
        ("refs reordered", &["@a.rs#L1-2", "@b.md"], "Per @b.md, fix @a.rs#L1-2", CLEAR),
        (
            "a quoted mention as given",
            &["@\"a b.rs#L1-2\""],
            "Fix @\"a b.rs#L1-2\" now.",
            CLEAR,
        ),
        ("a bare name with a quote", &["a\"b.rs:1-2"], "Fix a\"b.rs:1-2 now.", CLEAR),
        ("a bare name with a #", &["a#b.rs:1-2"], "Fix a#b.rs:1-2 now.", CLEAR),
        ("a trailing full stop", &["a.rs:1-2"], "Fix a.rs:1-2.", CLEAR),
        ("a trailing comma", &["@a.rs#L1-2", "@b.md"], "Fix @a.rs#L1-2, per @b.md.", CLEAR),
        ("a colon ending a clause", &["@a.rs#L1-2"], "in @a.rs#L1-2: fix it", CLEAR),
        (
            "in backticks and brackets",
            &["@a.rs#L1-2", "b.md"],
            "Fix `@a.rs#L1-2` (see [b.md])",
            CLEAR,
        ),
        ("named as given", &["@a.rs#L1-2"], "Fix @a.rs#L1-2", CLEAR),
        ("a bare ref given an @", &["a.rs:1-2"], "Fix @a.rs:1-2 now.", KEEP),
        ("in double quotes", &["@a.rs#L1-2"], "Fix \"@a.rs#L1-2\".", CLEAR),
    ];
    let wrong: Vec<&str> = rows
        .iter()
        .filter(|(_, forms, text, clears)| {
            let refs: Vec<RefItem> = forms.iter().map(|f| item(f, false, false)).collect();
            unnamed(text, &refs).is_none() != *clears
        })
        .map(|r| r.0)
        .collect();
    assert_eq!(wrong, [] as [&str; 0]);
}

/// Why a written result kept the basket, for the log: the first ref it does
/// not name, by its place in the basket, 1-based; `None` when it names them all.
#[test]
fn the_first_ref_not_named_is_found_by_its_place() {
    use cockpit_tui::panel::app::unnamed;
    let refs = [item("@a.rs#L1-2", false, false), item("@b.md", true, false)];
    assert_eq!(unnamed("fix @a.rs#L1-2 per @b.md", &refs), None);
    assert_eq!(unnamed("fix @a.rs#L1-2", &refs), Some(2));
    assert_eq!(unnamed("fix it", &refs), Some(1));
    assert_eq!(unnamed("fix it", &[]), None);
}

#[test]
fn a_failed_or_cancelled_write_leaves_the_basket() {
    let d = tree("failed");
    let mut a = compose(&d, true);
    ctrl(&mut a, 's');
    a.reply(Err(Fail::Failed("boom".into())));
    assert!(matches!(a.view, View::Error { .. }) && a.action.is_none() && a.sent_basket.is_none());
    let mut b = compose(&d, true);
    ctrl(&mut b, 's');
    b.key(KeyEvent::from(KeyCode::Esc));
    assert!(b.action.is_none() && b.sent_basket.is_none(), "{:?}", b.view);
}

/// A Fix corrects the draft and never sent the refs: its result carries none,
/// even when the draft named them all.
#[test]
fn a_fixed_draft_does_not_carry_the_basket() {
    let d = tree("fix");
    let mut a = compose(&d, true);
    a.draft = "fix it in @a.rs#L1-2 per @b.md".into();
    ctrl(&mut a, 'f');
    // a fix comes back as plain text, the draft corrected
    a.reply(Ok("Fix it in @a.rs#L1-2, per @b.md.".into()));
    assert!(matches!(a.view, View::Result { .. }), "{:?}", a.view);
    a.key(KeyEvent::from(KeyCode::Char('i')));
    assert!(a.action.is_some() && a.sent_basket.is_none(), "{:?}", a.sent_basket);
}

/// A command run from Steps is not the prompt the refs were picked for, even
/// one that names them all.
#[test]
fn a_step_does_not_carry_the_basket() {
    let d = tree("steps");
    let mut a = compose(&d, false);
    ctrl(&mut a, 's');
    a.reply(Ok("<command lang=\"bash\">wc -l a.rs:1-2 b.md</command>".into()));
    assert!(matches!(a.view, View::Steps { .. }), "{:?}", a.view);
    a.key(KeyEvent::from(KeyCode::Char('i')));
    assert!(a.action.is_some() && a.sent_basket.is_none(), "{:?} {:?}", a.view, a.sent_basket);
}

/// ^S from an answer's Compose is a run of its own (the thread ends) and its
/// request names the ask's lines, which `c` put in the basket.
#[test]
fn ctrl_s_from_an_answer_carries_the_asks_ref() {
    let d = tree("ask");
    let mut a = App::new("%3", cap(&d.0.join("repo")));
    a.claude = true;
    a.ask_open(cockpit_tui::panel::ask::load(&d.0.join("repo/a.rs"), 2, 3).unwrap());
    "why?".chars().for_each(|c| a.key(KeyEvent::from(KeyCode::Char(c))));
    a.key(KeyEvent::from(KeyCode::Enter));
    a.reply(Ok("Because.\n".into()));
    a.key(KeyEvent::from(KeyCode::Char('c')));
    ctrl(&mut a, 's');
    assert!(matches!(a.run.as_ref().map(|r| &r.flow), Some(Flow::Write { .. })));
    assert!(prompt(&a).contains("<ref form=\"mention\">@a.rs#L2-3</ref>"), "{}", prompt(&a));
}

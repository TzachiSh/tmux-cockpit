//! `cockpit-tui msg send` (#1575, epic #1573): one agent session's message to another,
//! kept in a private store under XDG state. docs/states-and-hooks.md, "Messages between sessions".
use crate::store::{create_file, xdg_dir};
use serde_json::json;
use std::fs::{DirBuilder, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const USAGE: &str = "usage: cockpit-tui msg send --session <id> <to> <summary-file> <body-file>";
/// The summary line's bytes, its newline not counted.
const SUMMARY_MAX: usize = 200;
/// What a send reads of the summary file: a line at the most, its newline, and one byte more
/// to tell a longer file. SUMMARY_MAX + 2, written out so it stays a constant.
const SUMMARY_READ: u64 = 202;
/// The calling pane's id and session in one display-message: a pane that is not there
/// answers with no id, or with another one.
const PANE_FMT: &str = "#{pane_id}\u{1f}#{@cockpit_session}";

/// What a command reads from outside. [`main`] hands it the real ones, a test its own.
trait World {
    fn var(&self, name: &str) -> Option<String>;
    fn tmux(&self, args: &[&str]) -> Option<String>;
    fn now(&self) -> SystemTime;
    fn random(&self) -> io::Result<[u8; 16]>;
}

struct Os;

impl World for Os {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    fn tmux(&self, args: &[&str]) -> Option<String> {
        crate::data::tmux(args)
    }

    fn now(&self) -> SystemTime {
        SystemTime::now()
    }

    fn random(&self) -> io::Result<[u8; 16]> {
        let mut b = [0; 16];
        File::open("/dev/urandom")?.read_exact(&mut b)?;
        Ok(b)
    }
}

#[derive(Debug, PartialEq)]
enum Fail {
    Usage,
    Refused(String),
}

impl From<String> for Fail {
    fn from(why: String) -> Fail {
        Fail::Refused(why)
    }
}

#[derive(Debug, PartialEq)]
enum Cmd {
    Send {
        session: String,
        to: String,
        summary: PathBuf,
        body: PathBuf,
    },
}

/// `cockpit-tui msg ...`: exit 0 done, 1 refused with one line on stderr, 2 usage.
pub fn main(args: &[String]) -> i32 {
    let done = run(args, &Os, &mut io::stdout().lock());
    exit_code(done, &mut io::stderr().lock())
}

/// The exit code, and on `err` the line that says why. A stderr that cannot take the
/// line leaves the code to say it.
fn exit_code(done: Result<(), Fail>, err: &mut dyn Write) -> i32 {
    let (code, line) = match done {
        Ok(()) => return 0,
        Err(Fail::Usage) => (2, USAGE.to_string()),
        Err(Fail::Refused(why)) => (1, format!("cockpit-tui: msg: {why}")),
    };
    let _ = writeln!(err, "{line}");
    code
}

fn parse(args: &[String]) -> Result<Cmd, Fail> {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a[..] {
        ["send", "--session", session, to, summary, body] => Ok(Cmd::Send {
            session: session.into(),
            to: to.into(),
            summary: summary.into(),
            body: body.into(),
        }),
        _ => Err(Fail::Usage),
    }
}

fn run(args: &[String], w: &dyn World, out: &mut dyn Write) -> Result<(), Fail> {
    let Cmd::Send {
        session,
        to,
        summary,
        body,
    } = parse(args)?;
    let me = caller(w, &session)?;
    let id = send(w, &me, &to, &summary, &body)?;
    let said = writeln!(out, "{id}").and_then(|()| out.flush());
    said.map_err(|e| format!("message {id} is stored, but its msg_id could not be printed: {e}"))?;
    Ok(())
}

/// The calling session (epic #1573, D9): the session the pane `$TMUX_PANE` publishes, which
/// `given` must be.
fn caller(w: &dyn World, given: &str) -> Result<String, String> {
    let pane = in_pane(w.var("TMUX").as_deref(), w.var("TMUX_PANE").as_deref())?;
    let answer = w.tmux(&["display-message", "-p", "-t", &pane, PANE_FMT]);
    let me = session_of(&pane, answer.as_deref())?;
    if given != me {
        return Err(format!("--session is not the session of pane {pane}"));
    }
    Ok(me)
}

/// The pane a run is in: `$TMUX` names its server, `$TMUX_PANE` the pane.
fn in_pane(tmux: Option<&str>, pane: Option<&str>) -> Result<String, String> {
    match (tmux, pane) {
        (Some(t), Some(p)) if !t.is_empty() && crate::panel::is_pane(p) => Ok(p.to_string()),
        _ => Err("not in a tmux pane: TMUX must be set and TMUX_PANE must be %N".into()),
    }
}

/// The session `pane` publishes, from tmux's answer to [`PANE_FMT`]. `None` is a tmux that
/// failed, which is a refusal of its own, never "no session".
fn session_of(pane: &str, answer: Option<&str>) -> Result<String, String> {
    let a = answer.ok_or_else(|| format!("cannot read pane {pane}: tmux failed"))?;
    let a = a.strip_suffix('\n').unwrap_or(a);
    let (id, value) = a.split_once('\u{1f}').unwrap_or((a, ""));
    if id != pane {
        return Err(format!("pane {pane} is not a pane of this tmux server"));
    }
    sid_hex(value).ok_or_else(|| format!("pane {pane} publishes no session in @cockpit_session"))
}

/// A session id as every contract id is written: the one form the hooks save
/// ([`crate::board::is_session_id`], #1589), its dashes dropped. `None` for any other value.
fn sid_hex(id: &str) -> Option<String> {
    crate::board::is_session_id(id).then(|| id.replace('-', ""))
}

/// Whether a pane of `list`, one `@cockpit_session` a line, publishes `to`.
fn publishes(list: &str, to: &str) -> bool {
    list.split('\n').filter_map(sid_hex).any(|s| s == to)
}

/// `tmux-cockpit` in the state folder, by the one XDG rule ([`xdg_dir`], #1508).
fn state_root(state: Option<&str>, home: Option<&str>) -> Result<PathBuf, String> {
    let base = xdg_dir(state, home, ".local/state");
    base.map(|b| b.join("tmux-cockpit"))
        .ok_or_else(|| "no state folder: neither XDG_STATE_HOME nor HOME is an absolute path".to_string())
}

/// One message from `me` to `to`, its msg_id returned. Every check that needs no folder
/// runs first, so a send refused by one makes none.
fn send(w: &dyn World, me: &str, to: &str, summary: &Path, body: &Path) -> Result<String, String> {
    let list = w.tmux(&["list-panes", "-a", "-F", "#{@cockpit_session}"]);
    let list = list.ok_or_else(|| "cannot list the panes of this tmux server: tmux failed".to_string())?;
    if !publishes(&list, to) {
        return Err("no pane of this tmux server publishes the session given as <to>".into());
    }
    let line = summary_line(&read_given(summary, "summary", SUMMARY_READ)?)?;
    let body = read_given(body, "body", u64::MAX)?;
    let id = msg_id(w.random().map_err(|e| format!("cannot read 16 random bytes: {e}"))?);
    let since = w
        .now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "the clock reads before 1970".to_string())?;
    let ts = crate::panel::changes::utc(since.as_secs());
    let root = state_root(w.var("XDG_STATE_HOME").as_deref(), w.var("HOME").as_deref())?;
    let txt = format!("{line}\n");
    store(
        &root,
        to,
        &id,
        [txt.as_bytes(), body.as_slice(), envelope(&id, me, to, &ts).as_bytes()],
    )?;
    Ok(id)
}

/// A file the caller names, at most `cap` bytes of it. A link is followed: the file is the
/// caller's own. O_NONBLOCK opens a FIFO at once, and it is refused as no regular file.
fn read_given(path: &Path, what: &str, cap: u64) -> Result<Vec<u8>, String> {
    let at = |e: io::Error| format!("cannot read the {what} file {}: {e}", shown(path));
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .map_err(at)?;
    if !f.metadata().map_err(at)?.is_file() {
        return Err(format!("the {what} file {} is not a regular file", shown(path)));
    }
    let mut b = Vec::new();
    f.take(cap).read_to_end(&mut b).map_err(at)?;
    Ok(b)
}

/// The summary: one line of UTF-8, a final newline allowed, not empty, at most
/// [`SUMMARY_MAX`] bytes, and nothing [`crate::pick::clean`] would change: no tab, control
/// or bidi character.
fn summary_line(b: &[u8]) -> Result<String, String> {
    let b = b.strip_suffix(b"\n").unwrap_or(b);
    if b.is_empty() {
        return Err("the summary is empty".into());
    }
    if b.len() > SUMMARY_MAX {
        return Err(format!("the summary is over {SUMMARY_MAX} bytes"));
    }
    let s = std::str::from_utf8(b).map_err(|_| "the summary is not UTF-8".to_string())?;
    if crate::pick::clean(s) != s {
        return Err("the summary is not one line of plain text".into());
    }
    Ok(s.to_string())
}

/// 16 random bytes as 32 lowercase hex.
fn msg_id(r: [u8; 16]) -> String {
    r.iter().map(|b| format!("{b:02x}")).collect()
}

/// The envelope, one line of contracts/agent-message.schema.json version 2: ids and refs
/// only. A message starts its own chain, so its correlation_id is its msg_id.
fn envelope(id: &str, from: &str, to: &str, ts: &str) -> String {
    let v = json!({
        "schema_version": 2,
        "msg_id": id,
        "from": from,
        "to": to,
        "subject": format!("msg.{from}.{to}"),
        "timestamp": ts,
        "correlation_id": id,
        "payload": { "body_ref": format!("{id}.md"), "summary_ref": format!("{id}.txt") }
    });
    format!("{v}\n")
}

/// The summary and the body as refs, then the envelope, so an envelope names only refs in
/// place. A refusal takes back the files this call linked, and only those.
fn store(root: &Path, to: &str, id: &str, [txt, md, json]: [&[u8]; 3]) -> Result<(), String> {
    let made = DirBuilder::new().recursive(true).mode(0o700).create(root);
    made.map_err(|e| format!("cannot make {}: {e}", shown(root)))?;
    let (refs, msg) = (root.join("refs"), root.join("msg"));
    let inbox = msg.join(to);
    // SAFETY: geteuid takes no argument and cannot fail.
    let me = unsafe { libc::geteuid() };
    for dir in [&refs, &msg, &inbox] {
        own_dir(dir, me)?;
    }
    let files = [
        (&refs, format!("{id}.txt"), txt),
        (&refs, format!("{id}.md"), md),
        (&inbox, format!("{id}.json"), json),
    ];
    let mut linked = Vec::new();
    for (dir, name, bytes) in files {
        let why = match put(dir, &name, bytes) {
            Ok(true) => {
                linked.push(dir.join(name));
                continue;
            }
            Ok(false) => format!("msg_id {id} is already stored"),
            Err(why) => why,
        };
        return Err(take_back(why, &linked));
    }
    Ok(())
}

/// `bytes` as the new file `dir/name`, through `.<name>.<pid>.new` beside it. `Ok(false)` when
/// the name is taken.
fn put(dir: &Path, name: &str, bytes: &[u8]) -> Result<bool, String> {
    let tmp = dir.join(format!(".{name}.{}.new", std::process::id()));
    create_file(dir, &tmp, &dir.join(name), bytes).map_err(|(p, e)| format!("cannot write {}: {e}", shown(&p)))
}

/// `why`, once each of `linked` is removed; a file that stays is named in the line.
fn take_back(why: String, linked: &[PathBuf]) -> String {
    linked.iter().fold(why, |why, p| match std::fs::remove_file(p) {
        Ok(()) => why,
        Err(e) => format!("{why}; {} stays: {e}", shown(p)),
    })
}

/// A path as a refusal line shows it: [`crate::pick::clean`] drops a newline, a control or a
/// bidi character, so the line stays one line.
fn shown(p: &Path) -> String {
    crate::pick::clean(&p.display().to_string())
}

/// A folder of the store, made 0700 when missing. One already there must be a folder of user
/// `me`, not a link, with no group or other bits.
fn own_dir(dir: &Path, me: u32) -> Result<(), String> {
    match DirBuilder::new().mode(0o700).create(dir) {
        Err(e) if e.kind() != io::ErrorKind::AlreadyExists => {
            return Err(format!("cannot make {}: {e}", shown(dir)));
        }
        _ => {}
    }
    let m = std::fs::symlink_metadata(dir).map_err(|e| format!("cannot read {}: {e}", shown(dir)))?;
    match flaw(m.file_type().is_symlink(), m.is_dir(), m.uid(), m.mode(), me) {
        Some(why) => Err(format!("{} is not a private folder: it {why}", shown(dir))),
        None => Ok(()),
    }
}

/// Why a folder of the store is no private folder of user `me`, or `None`.
fn flaw(link: bool, dir: bool, uid: u32, mode: u32, me: u32) -> Option<String> {
    if link {
        Some("is a symbolic link".into())
    } else if !dir {
        Some("is not a folder".into())
    } else if uid != me {
        Some(format!("belongs to uid {uid}"))
    } else if mode & 0o077 != 0 {
        Some(format!("has mode {:o}", mode & 0o777))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::cell::RefCell;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::time::Duration;

    /// Pane %1 publishes A, pane %2 publishes B.
    const UA: &str = "3f2a9c0d-1e4b-4a67-8c9d-0e1f2a3b4c5d";
    const HA: &str = "3f2a9c0d1e4b4a678c9d0e1f2a3b4c5d";
    const UB: &str = "7a1b2c3d-4e5f-4061-8273-94a5b6c7d8e9";
    const HB: &str = "7a1b2c3d4e5f4061827394a5b6c7d8e9";
    const RAND: [u8; 16] = [
        0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
    ];
    const ID: &str = "00112233445566778899aabbccddeeff";
    /// 2026-11-28T08:07:05Z: 20785 days and 29225 seconds after the epoch.
    const NOW: u64 = 20_785 * 86_400 + 8 * 3600 + 7 * 60 + 5;
    const ENVELOPE: &str = concat!(
        r#"{"schema_version":2,"msg_id":"00112233445566778899aabbccddeeff","#,
        r#""from":"3f2a9c0d1e4b4a678c9d0e1f2a3b4c5d","to":"7a1b2c3d4e5f4061827394a5b6c7d8e9","#,
        r#""subject":"msg.3f2a9c0d1e4b4a678c9d0e1f2a3b4c5d.7a1b2c3d4e5f4061827394a5b6c7d8e9","#,
        r#""timestamp":"2026-11-28T08:07:05Z","correlation_id":"00112233445566778899aabbccddeeff","#,
        r#""payload":{"body_ref":"00112233445566778899aabbccddeeff.md","#,
        r#""summary_ref":"00112233445566778899aabbccddeeff.txt"}}"#,
        "\n"
    );

    /// A test's own folder under TMPDIR, removed when it drops.
    struct Tmp(PathBuf);

    impl Tmp {
        fn new(name: &str) -> Tmp {
            let d = std::env::temp_dir().join(format!("cockpit-msg-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            Tmp(d)
        }

        /// The store's root, tmux-cockpit under this test's XDG_STATE_HOME.
        fn root(&self) -> PathBuf {
            self.0.join("state/tmux-cockpit")
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    impl std::ops::Deref for Tmp {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }

    struct Fake {
        vars: Vec<(&'static str, String)>,
        /// display-message's answer, and list-panes'
        pane: Option<String>,
        list: Option<String>,
        random: Option<[u8; 16]>,
        now: SystemTime,
        calls: RefCell<Vec<String>>,
    }

    impl World for Fake {
        fn var(&self, name: &str) -> Option<String> {
            self.vars.iter().find(|(k, _)| *k == name).map(|(_, v)| v.clone())
        }

        fn tmux(&self, args: &[&str]) -> Option<String> {
            self.calls.borrow_mut().push(args.join(" "));
            match args.first() {
                Some(&"display-message") => self.pane.clone(),
                Some(&"list-panes") => self.list.clone(),
                _ => None,
            }
        }

        fn now(&self) -> SystemTime {
            self.now
        }

        fn random(&self) -> io::Result<[u8; 16]> {
            self.random.ok_or_else(|| io::Error::other("no entropy"))
        }
    }

    /// A run from pane %1, which publishes A, on a server that lists A and B.
    fn world(t: &Path) -> Fake {
        Fake {
            vars: vec![
                ("TMUX", "/s/default,1,0".into()),
                ("TMUX_PANE", "%1".into()),
                ("XDG_STATE_HOME", t.join("state").display().to_string()),
            ],
            pane: Some(format!("%1\u{1f}{UA}\n")),
            list: Some(format!("{UA}\n{UB}\n\n")),
            random: Some(RAND),
            now: UNIX_EPOCH + Duration::from_secs(NOW),
            calls: RefCell::default(),
        }
    }

    fn go(w: &dyn World, args: &[&str]) -> (Result<(), Fail>, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let mut out = Vec::new();
        let done = run(&args, w, &mut out);
        (done, String::from_utf8(out).unwrap())
    }

    fn no(why: &str) -> Result<(), Fail> {
        Err(Fail::Refused(why.to_string()))
    }

    /// A send from A to B of the summary and the body given, from files in the test's folder.
    fn send_ab(w: &Fake, t: &Path, summary: &str, body: &str) -> (Result<(), Fail>, String) {
        std::fs::write(t.join("sum"), summary).unwrap();
        std::fs::write(t.join("body"), body).unwrap();
        let (s, b) = (t.join("sum").display().to_string(), t.join("body").display().to_string());
        go(w, &["send", "--session", HA, HB, &s, &b])
    }

    /// Every path under `dir`, relative and sorted, so a test compares the whole store.
    fn tree(dir: &Path) -> Vec<String> {
        fn walk(base: &Path, d: &Path, v: &mut Vec<String>) {
            for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
                v.push(e.path().strip_prefix(base).unwrap().display().to_string());
                if e.file_type().unwrap().is_dir() {
                    walk(base, &e.path(), v);
                }
            }
        }
        let mut v = vec![];
        walk(dir, dir, &mut v);
        v.sort();
        v
    }

    fn mode(p: &Path) -> u32 {
        std::fs::symlink_metadata(p).unwrap().mode() & 0o777
    }

    fn text(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap()
    }

    fn os_error(code: i32) -> String {
        io::Error::from_raw_os_error(code).to_string()
    }

    /// A stdout that takes nothing.
    struct Shut;

    impl Write for Shut {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("closed"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("closed"))
        }
    }

    #[test]
    fn parse_takes_send_in_its_one_shape() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let send = Cmd::Send {
            session: "s".into(),
            to: "t".into(),
            summary: "f".into(),
            body: "b".into(),
        };
        assert_eq!(parse(&args(&["send", "--session", "s", "t", "f", "b"])), Ok(send));
        let bads: [&[&str]; 6] = [
            &[],
            &["send"],
            &["send", "--session", "s", "t", "f"],
            &["send", "--session", "s", "t", "f", "b", "x"],
            &["send", "--sess", "s", "t", "f", "b"],
            &["who"],
        ];
        for bad in bads {
            assert_eq!(parse(&args(bad)), Err(Fail::Usage), "{bad:?}");
        }
    }

    #[test]
    fn exit_code_is_0_1_or_2_with_the_line_that_says_why() {
        let mut err = Vec::new();
        assert_eq!(exit_code(Ok(()), &mut err), 0);
        assert!(err.is_empty());
        assert_eq!(exit_code(Err(Fail::Usage), &mut err), 2);
        assert_eq!(
            String::from_utf8(std::mem::take(&mut err)).unwrap(),
            "usage: cockpit-tui msg send --session <id> <to> <summary-file> <body-file>\n"
        );
        assert_eq!(exit_code(Err(Fail::Refused("why".into())), &mut err), 1);
        assert_eq!(String::from_utf8(err).unwrap(), "cockpit-tui: msg: why\n");
        // a stderr that takes nothing leaves the code
        assert_eq!(exit_code(Err(Fail::Refused("why".into())), &mut Shut), 1);
    }

    #[test]
    fn in_pane_needs_a_tmux_and_a_pane_id() {
        let no = Err("not in a tmux pane: TMUX must be set and TMUX_PANE must be %N".to_string());
        assert_eq!(in_pane(Some("/s,1,0"), Some("%12")), Ok("%12".to_string()));
        assert_eq!(in_pane(None, Some("%12")), no);
        assert_eq!(in_pane(Some(""), Some("%12")), no);
        assert_eq!(in_pane(Some("/s,1,0"), None), no);
        assert_eq!(in_pane(Some("/s,1,0"), Some("12")), no);
        assert_eq!(in_pane(Some("/s,1,0"), Some("%")), no);
    }

    #[test]
    fn sid_hex_drops_the_dashes_of_the_one_form_the_hooks_save() {
        assert_eq!(sid_hex(UA).as_deref(), Some(HA));
        assert_eq!(sid_hex(UB).as_deref(), Some(HB));
        for bad in [
            "",
            "not-a-uuid",
            HA,
            "3F2A9C0D-1E4B-4A67-8C9D-0E1F2A3B4C5D",
            "3f2a9c0d-1e4b-4a67-8c9d-0e1f2a3b4c5D",
            "3F2A9C0D-1E4B-4A67-8C9D",
            "3F2A9C0D-1E4B-4A67-8C9D-0E1F2A3B4C5D-00",
            "3F2A9C0D1-E4B-4A67-8C9D-0E1F2A3B4C5D",
            "3F2A9C0D-1E4B-4A67-8C9D-0E1F2A3B4C5DE",
            "3G2A9C0D-1E4B-4A67-8C9D-0E1F2A3B4C5D",
            " 3F2A9C0D-1E4B-4A67-8C9D-0E1F2A3B4C5",
        ] {
            assert_eq!(sid_hex(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn session_of_reads_the_pane_id_and_its_session() {
        assert_eq!(session_of("%1", Some(format!("%1\u{1f}{UA}\n").as_str())), Ok(HA.to_string()));
        assert_eq!(session_of("%1", Some(format!("%1\u{1f}{UA}").as_str())), Ok(HA.to_string()));
        let tmux = Err("cannot read pane %1: tmux failed".to_string());
        assert_eq!(session_of("%1", None), tmux);
        let gone = Err("pane %1 is not a pane of this tmux server".to_string());
        assert_eq!(session_of("%1", Some("\u{1f}\n")), gone);
        assert_eq!(session_of("%1", Some(format!("%2\u{1f}{UA}\n").as_str())), gone);
        assert_eq!(
            session_of("%1", Some("%1\n")),
            Err("pane %1 publishes no session in @cockpit_session".into())
        );
        let none = Err("pane %1 publishes no session in @cockpit_session".to_string());
        assert_eq!(session_of("%1", Some("%1\u{1f}\n")), none);
        assert_eq!(session_of("%1", Some("%1\u{1f}not-a-uuid\n")), none);
    }

    #[test]
    fn caller_asks_tmux_for_the_pane_once_and_holds_the_given_id_to_it() {
        let t = Tmp::new("caller");
        let w = world(&t);
        assert_eq!(caller(&w, HA), Ok(HA.to_string()));
        assert_eq!(*w.calls.borrow(), ["display-message -p -t %1 #{pane_id}\u{1f}#{@cockpit_session}"]);
        let other = Err("--session is not the session of pane %1".to_string());
        assert_eq!(caller(&w, HB), other);
        assert_eq!(caller(&w, "x\ny"), other, "no id the caller gave is echoed");
        let mut w = world(&t);
        w.vars.remove(0);
        assert_eq!(
            caller(&w, HA),
            Err("not in a tmux pane: TMUX must be set and TMUX_PANE must be %N".into())
        );
        assert!(w.calls.borrow().is_empty(), "no tmux call before the pane is known");
    }

    #[test]
    fn publishes_reads_only_uuids_off_the_list() {
        let list = format!("{UA}\n\nnot-a-uuid\n{HB}\n");
        assert!(publishes(&list, HA));
        assert!(!publishes(&list, HB), "a bare hex id is no UUID");
        assert!(publishes(&format!("{UB}\n"), HB));
        assert!(
            !publishes(&UB.to_uppercase(), HB),
            "an upper-case UUID is not the form the hooks save"
        );
        assert!(!publishes("", HA));
    }

    #[test]
    fn state_root_is_tmux_cockpit_by_the_xdg_rule() {
        assert_eq!(state_root(Some("/x/state"), Some("/home/u")), Ok("/x/state/tmux-cockpit".into()));
        assert_eq!(
            state_root(Some("rel"), Some("/home/u")),
            Ok("/home/u/.local/state/tmux-cockpit".into())
        );
        let none = Err("no state folder: neither XDG_STATE_HOME nor HOME is an absolute path".to_string());
        assert_eq!(state_root(None, Some("home")), none);
        assert_eq!(state_root(None, None), none);
    }

    #[test]
    fn summary_line_is_one_line_of_plain_utf8_up_to_200_bytes() {
        assert_eq!(SUMMARY_READ, SUMMARY_MAX as u64 + 2);
        let y200 = "y".repeat(200);
        assert_eq!(summary_line(b"hello\n"), Ok("hello".into()));
        assert_eq!(summary_line(b"hello"), Ok("hello".into()));
        assert_eq!(summary_line("caf\u{e9} \u{2014} ok".as_bytes()), Ok("caf\u{e9} \u{2014} ok".into()));
        assert_eq!(summary_line(format!("{y200}\n").as_bytes()), Ok(y200.clone()));
        assert_eq!(summary_line(y200.as_bytes()), Ok(y200.clone()));
        let empty = Err("the summary is empty".to_string());
        assert_eq!(summary_line(b""), empty);
        assert_eq!(summary_line(b"\n"), empty);
        let over = Err("the summary is over 200 bytes".to_string());
        assert_eq!(summary_line("y".repeat(201).as_bytes()), over);
        assert_eq!(summary_line(format!("{y200}\nx").as_bytes()), over);
        assert_eq!(summary_line(b"\xff\n"), Err("the summary is not UTF-8".into()));
        let plain = Err("the summary is not one line of plain text".to_string());
        for bad in [
            "a\n\n",
            "a\nb",
            "a\tb",
            "a\0b",
            "a\x1b[31mb",
            "a\rb",
            "a\u{7f}b",
            "a\u{85}b",
            "a\u{202e}b",
        ] {
            assert_eq!(summary_line(bad.as_bytes()), plain, "{bad:?}");
        }
    }

    #[test]
    fn msg_id_is_the_16_bytes_as_32_lowercase_hex() {
        assert_eq!(msg_id(RAND), ID);
        assert_eq!(msg_id([0x0f; 16]), "0f".repeat(16));
    }

    #[test]
    fn envelope_holds_the_keys_the_schema_requires_and_no_other() {
        let schema: Value = serde_json::from_str(include_str!("../../contracts/agent-message.schema.json")).unwrap();
        assert_eq!(schema["properties"]["schema_version"]["const"], 2);
        let v: Value = serde_json::from_str(ENVELOPE).unwrap();
        let keys = |o: &Value| o.as_object().unwrap().keys().cloned().collect::<std::collections::BTreeSet<_>>();
        let required: std::collections::BTreeSet<String> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|k| k.as_str().unwrap().to_string())
            .collect();
        assert_eq!(keys(&v), required);
        assert!(keys(&v["payload"]).is_subset(&keys(&schema["properties"]["payload"]["properties"])));
    }

    #[test]
    fn read_given_reads_a_regular_file_up_to_its_cap() {
        let t = Tmp::new("given");
        std::fs::write(t.join("f"), "0123456789").unwrap();
        assert_eq!(read_given(&t.join("f"), "body", 4), Ok(b"0123".to_vec()));
        assert_eq!(read_given(&t.join("f"), "body", u64::MAX), Ok(b"0123456789".to_vec()));
        let missing = format!(
            "cannot read the summary file {}: {}",
            t.join("no").display(),
            os_error(libc::ENOENT)
        );
        assert_eq!(read_given(&t.join("no"), "summary", 9), Err(missing.clone()));
        assert_eq!(read_given(&t.join("n\no"), "summary", 9), Err(missing), "a newline is dropped");
        let dir = Err(format!("the body file {} is not a regular file", t.display()));
        assert_eq!(read_given(&t, "body", 9), dir);
        let fifo = t.join("fifo");
        let c = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: mkfifo on a NUL-terminated path this test owns.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let fifo_no = Err(format!("the body file {} is not a regular file", fifo.display()));
        assert_eq!(read_given(&fifo, "body", 9), fifo_no, "opened at once, and refused");
    }

    #[test]
    fn flaw_names_why_a_folder_is_not_private() {
        assert_eq!(flaw(false, true, 7, 0o40700, 7), None);
        assert_eq!(flaw(true, false, 7, 0o120777, 7).as_deref(), Some("is a symbolic link"));
        assert_eq!(flaw(false, false, 7, 0o100600, 7).as_deref(), Some("is not a folder"));
        assert_eq!(flaw(false, true, 8, 0o40700, 7).as_deref(), Some("belongs to uid 8"));
        assert_eq!(flaw(false, true, 7, 0o40755, 7).as_deref(), Some("has mode 755"));
        assert_eq!(flaw(false, true, 7, 0o40701, 7).as_deref(), Some("has mode 701"));
        assert_eq!(flaw(false, true, 7, 0o40710, 7).as_deref(), Some("has mode 710"));
        assert_eq!(flaw(false, true, 7, 0o40500, 7), None);
    }

    #[test]
    fn own_dir_refuses_a_link_a_file_and_a_folder_others_can_enter() {
        let t = Tmp::new("ownno");
        let (link, f, wide) = (t.join("link"), t.join("f"), t.join("wide"));
        let me = std::fs::metadata(&*t).unwrap().uid();
        own_dir(&t.join("real"), me).unwrap();
        let real = Err(format!(
            "{} is not a private folder: it belongs to uid {me}",
            t.join("real").display()
        ));
        assert_eq!(own_dir(&t.join("real"), me + 1), real);
        symlink(t.join("real"), &link).unwrap();
        assert_eq!(
            own_dir(&link, me),
            Err(format!("{} is not a private folder: it is a symbolic link", link.display()))
        );
        std::fs::write(&f, "").unwrap();
        assert_eq!(
            own_dir(&f, me),
            Err(format!("{} is not a private folder: it is not a folder", f.display()))
        );
        std::fs::create_dir(&wide).unwrap();
        std::fs::set_permissions(&wide, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            own_dir(&wide, me),
            Err(format!("{} is not a private folder: it has mode 755", wide.display()))
        );
        let under = f.join("x");
        assert_eq!(
            own_dir(&under, me),
            Err(format!("cannot make {}: {}", under.display(), os_error(libc::ENOTDIR)))
        );
    }

    #[test]
    fn create_file_links_a_new_0600_file_and_never_replaces_one() {
        let t = Tmp::new("create");
        let (tmp, path) = (t.join(".a.new"), t.join("a"));
        assert_eq!(create_file(&t, &tmp, &path, b"one").map_err(|e| e.1.to_string()), Ok(true));
        assert_eq!((text(&path), mode(&path), tmp.exists()), ("one".into(), 0o600, false));
        assert_eq!(create_file(&t, &tmp, &path, b"two").map_err(|e| e.1.to_string()), Ok(false));
        assert_eq!((text(&path), tmp.exists()), ("one".into(), false));
        let dangling = t.join("b");
        symlink(t.join("nowhere"), &dangling).unwrap();
        assert_eq!(create_file(&t, &tmp, &dangling, b"x").map_err(|e| e.1.to_string()), Ok(false));
        assert!(std::fs::symlink_metadata(&dangling).unwrap().file_type().is_symlink());
        assert!(!t.join("nowhere").exists() && !tmp.exists());
        // a killed writer's leftover temp file is no obstacle
        std::fs::write(&tmp, "old").unwrap();
        assert_eq!(create_file(&t, &tmp, &t.join("c"), b"new").map_err(|e| e.1.to_string()), Ok(true));
        assert_eq!(text(&t.join("c")), "new");
    }

    #[test]
    fn create_file_names_the_path_it_failed_at() {
        let t = Tmp::new("createno");
        let (tmp, path) = (t.join(".a.new"), t.join("none/a"));
        let e = create_file(&t, &tmp, &path, b"x").unwrap_err();
        assert_eq!((e.0, e.1.raw_os_error()), (path, Some(libc::ENOENT)));
        assert!(!tmp.exists());
        let tmp = t.join("none/.a.new");
        let e = create_file(&t, &tmp, &t.join("a"), b"x").unwrap_err();
        assert_eq!((e.0, e.1.raw_os_error()), (tmp, Some(libc::ENOENT)));
        // a leftover that cannot be removed is a refusal, not a create over it
        let tmp = t.join(".d.new");
        std::fs::create_dir_all(tmp.join("x")).unwrap();
        let e = create_file(&t, &tmp, &t.join("d"), b"x").unwrap_err();
        assert_eq!(e.0, tmp);
        assert_ne!(e.1.kind(), io::ErrorKind::AlreadyExists, "{}", e.1);
    }

    #[test]
    fn send_stores_the_refs_and_the_envelope_and_prints_the_msg_id() {
        let t = Tmp::new("send");
        let w = world(&t);
        assert_eq!(
            send_ab(&w, &t, "hello from A\n", "line one\n\tline two\n"),
            (Ok(()), format!("{ID}\n"))
        );
        let r = t.root();
        let want = [
            "msg".to_string(),
            format!("msg/{HB}"),
            format!("msg/{HB}/{ID}.json"),
            "refs".into(),
            format!("refs/{ID}.md"),
            format!("refs/{ID}.txt"),
        ];
        assert_eq!(tree(&r), want);
        assert_eq!(text(&r.join(format!("refs/{ID}.txt"))), "hello from A\n");
        assert_eq!(text(&r.join(format!("refs/{ID}.md"))), "line one\n\tline two\n");
        assert_eq!(text(&r.join(format!("msg/{HB}/{ID}.json"))), ENVELOPE);
        for f in [format!("refs/{ID}.txt"), format!("refs/{ID}.md"), format!("msg/{HB}/{ID}.json")] {
            assert_eq!(mode(&r.join(&f)), 0o600, "{f}");
        }
        for d in ["refs".to_string(), "msg".into(), format!("msg/{HB}")] {
            assert_eq!(mode(&r.join(&d)), 0o700, "{d}");
        }
        let calls = [
            "display-message -p -t %1 #{pane_id}\u{1f}#{@cockpit_session}",
            "list-panes -a -F #{@cockpit_session}",
        ];
        assert_eq!(*w.calls.borrow(), calls);
    }

    #[test]
    fn a_root_another_tool_made_0755_keeps_its_mode_and_the_store_stays_private() {
        let t = Tmp::new("root755");
        std::fs::create_dir_all(t.root()).unwrap();
        std::fs::set_permissions(t.root(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(send_ab(&world(&t), &t, "hi\n", "b\n").0, Ok(()));
        assert_eq!(
            (mode(&t.root()), mode(&t.root().join("refs")), mode(&t.root().join("msg"))),
            (0o755, 0o700, 0o700)
        );
    }

    #[test]
    fn a_new_root_is_made_0700_with_its_missing_parents() {
        let t = Tmp::new("rootnew");
        assert_eq!(send_ab(&world(&t), &t, "hi\n", "b\n").0, Ok(()));
        assert_eq!((mode(&t.join("state")), mode(&t.root())), (0o700, 0o700));
    }

    #[test]
    fn a_store_folder_others_can_enter_is_a_refusal() {
        let t = Tmp::new("refs755");
        let refs = t.root().join("refs");
        std::fs::create_dir_all(&refs).unwrap();
        std::fs::set_permissions(&refs, std::fs::Permissions::from_mode(0o755)).unwrap();
        let why = format!("{} is not a private folder: it has mode 755", refs.display());
        assert_eq!(send_ab(&world(&t), &t, "hi\n", "b\n"), (no(&why), String::new()));
        assert_eq!(tree(&t.root()), ["refs"]);
        let root = t.join("file");
        std::fs::write(&root, "").unwrap();
        let mut w = world(&t);
        w.vars[2].1 = root.display().to_string();
        let why = format!("cannot make {}: {}", root.join("tmux-cockpit").display(), os_error(libc::ENOTDIR));
        assert_eq!(send_ab(&w, &t, "hi\n", "b\n").0, no(&why));
    }

    #[test]
    fn send_refuses_before_it_makes_any_folder() {
        let t = Tmp::new("sendno");
        let w = world(&t);
        assert_eq!(send_ab(&w, &t, "\n", "b\n").0, no("the summary is empty"));
        let mut w = world(&t);
        w.list = None;
        assert_eq!(
            send_ab(&w, &t, "hi\n", "b\n").0,
            no("cannot list the panes of this tmux server: tmux failed")
        );
        let mut w = world(&t);
        w.list = Some(format!("{UA}\n"));
        let unknown = no("no pane of this tmux server publishes the session given as <to>");
        assert_eq!(send_ab(&w, &t, "hi\n", "b\n").0, unknown);
        let (s, b) = (t.join("sum").display().to_string(), t.join("body").display().to_string());
        assert_eq!(go(&w, &["send", "--session", HA, "x\ny", &s, &b]).0, unknown, "<to> is not echoed");
        let over = no("the summary is over 200 bytes");
        assert_eq!(send_ab(&world(&t), &t, &"y".repeat(201), "b\n").0, over, "the read takes 202 bytes");
        let mut w = world(&t);
        w.random = None;
        assert_eq!(send_ab(&w, &t, "hi\n", "b\n").0, no("cannot read 16 random bytes: no entropy"));
        let mut w = world(&t);
        w.now = UNIX_EPOCH - Duration::from_secs(1);
        assert_eq!(send_ab(&w, &t, "hi\n", "b\n").0, no("the clock reads before 1970"));
        let mut w = world(&t);
        w.vars[2].1 = "relative".into();
        let none = "no state folder: neither XDG_STATE_HOME nor HOME is an absolute path";
        assert_eq!(send_ab(&w, &t, "hi\n", "b\n").0, no(none));
        let mut w = world(&t);
        w.pane = None;
        assert_eq!(send_ab(&w, &t, "hi\n", "b\n").0, no("cannot read pane %1: tmux failed"));
        assert!(!t.join("state").exists());
    }

    #[test]
    fn a_taken_msg_id_is_refused_and_only_this_sends_files_are_taken_back() {
        let t = Tmp::new("taken");
        let r = t.root();
        std::fs::create_dir_all(r.join("refs")).unwrap();
        std::fs::set_permissions(r.join("refs"), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(r.join(format!("refs/{ID}.md")), "theirs").unwrap();
        let taken = format!("msg_id {ID} is already stored");
        assert_eq!(send_ab(&world(&t), &t, "hi\n", "b\n"), (no(&taken), String::new()));
        assert_eq!(
            tree(&r),
            ["msg".to_string(), format!("msg/{HB}"), "refs".into(), format!("refs/{ID}.md")]
        );
        assert_eq!(text(&r.join(format!("refs/{ID}.md"))), "theirs");
    }

    #[test]
    fn the_refs_are_linked_before_the_envelope_and_a_failed_write_takes_back_the_ones_linked() {
        // each blocked step says so in its own words, so the refusal shows which step came first
        let t = Tmp::new("order");
        let r = t.root();
        let temp = |name: &str| r.join("refs").join(format!(".{ID}.{name}.{}.new", std::process::id()));
        for name in ["txt", "md"] {
            std::fs::create_dir_all(temp(name).join("x")).unwrap();
        }
        std::fs::create_dir_all(r.join(format!("msg/{HB}"))).unwrap();
        std::fs::write(r.join(format!("msg/{HB}/{ID}.json")), "theirs").unwrap();
        for d in ["refs".to_string(), "msg".into(), format!("msg/{HB}")] {
            std::fs::set_permissions(r.join(d), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let unlink = if cfg!(target_os = "linux") { libc::EISDIR } else { libc::EPERM };
        let blocked = |name: &str| no(&format!("cannot write {}: {}", temp(name).display(), os_error(unlink)));
        assert_eq!(send_ab(&world(&t), &t, "hi\n", "b\n").0, blocked("txt"));
        std::fs::remove_dir_all(temp("txt")).unwrap();
        assert_eq!(send_ab(&world(&t), &t, "hi\n", "b\n").0, blocked("md"));
        assert!(
            !r.join(format!("refs/{ID}.txt")).exists(),
            "the summary this send linked is taken back"
        );
        std::fs::remove_dir_all(temp("md")).unwrap();
        assert_eq!(
            send_ab(&world(&t), &t, "hi\n", "b\n").0,
            no(&format!("msg_id {ID} is already stored"))
        );
        assert_eq!(tree(&r.join("refs")), Vec::<String>::new());
        assert_eq!(text(&r.join(format!("msg/{HB}/{ID}.json"))), "theirs");
    }

    #[test]
    fn take_back_names_a_file_it_cannot_remove() {
        let t = Tmp::new("takeback");
        let (mine, gone) = (t.join("mine"), t.join("gone"));
        std::fs::write(&mine, "").unwrap();
        let why = take_back("why".into(), &[mine.clone(), gone.clone()]);
        assert_eq!(why, format!("why; {} stays: {}", gone.display(), os_error(libc::ENOENT)));
        assert!(!mine.exists());
        assert_eq!(take_back("why".into(), &[]), "why");
    }

    #[test]
    fn a_msg_id_that_cannot_be_printed_leaves_the_message_stored() {
        let t = Tmp::new("noprint");
        std::fs::write(t.join("sum"), "hi\n").unwrap();
        std::fs::write(t.join("body"), "b\n").unwrap();
        let (s, b) = (t.join("sum").display().to_string(), t.join("body").display().to_string());
        let args: Vec<String> = ["send", "--session", HA, HB, &s, &b].iter().map(|a| a.to_string()).collect();
        let why = format!("message {ID} is stored, but its msg_id could not be printed: closed");
        assert_eq!(run(&args, &world(&t), &mut Shut), no(&why));
        assert!(t.root().join(format!("msg/{HB}/{ID}.json")).is_file());
    }
}

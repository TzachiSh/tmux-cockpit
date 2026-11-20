//! #462: the trees a session's own tool calls reached. Claude Code keeps a session's cwd
//! where it launched, so a `cd X && ...` in one Bash call moves neither the hooks' cwd nor
//! the transcript's. The fold ([`super::fold`]) hands each main-thread tool call to
//! [`dir_of`], which reads the folder it names without touching the disk; [`recent`] turns
//! those folders into git toplevels with their ages, for `workdir::Candidates::active`.
//! No git fork: a toplevel is found by walking up to a `.git`, and kept for [`TTL`].
use super::workdir::Recent;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

/// How many distinct folders one transcript's fold keeps, the newest of each.
pub(super) const KEEP: usize = 8;
/// How long a walk's answer, a tree or none, is kept.
const TTL: Duration = Duration::from_secs(10);
/// Past this many folders the memory drops what is past its [`TTL`], then all of it.
const CACHE_MAX: usize = 256;
/// A command is read this far; the folder a call works in is named up front.
const SCAN_MAX: usize = 16 * 1024;
/// A longer word is no path.
const WORD_MAX: usize = 4096;

/// The folder a tool call works in, absolute and normalised, or None. Bash: the first
/// command of the call that is a `cd`, a `pushd` or a `git -C`. Edit, MultiEdit, Write,
/// Read: the folder of `file_path`; NotebookEdit: of `notebook_path`. A relative path is
/// read against the row's `cwd`; `~` and a leading `$HOME` against `home`.
pub(super) fn dir_of(name: &str, input: &Value, cwd: &str, home: &str) -> Option<String> {
    let path = |k: &str| input[k].as_str().filter(|p| !p.is_empty());
    match name {
        "Bash" => bash_dir(path("command")?, cwd, home),
        "Edit" | "MultiEdit" | "Write" | "Read" => parent(&resolve(path("file_path")?, cwd)?),
        "NotebookEdit" => parent(&resolve(path("notebook_path")?, cwd)?),
        _ => None,
    }
}

/// `p` absolute: as it is when it starts with `/`, else under `cwd`, which must be.
fn resolve(p: &str, cwd: &str) -> Option<String> {
    match (p.starts_with('/'), cwd.starts_with('/')) {
        (true, _) => normal(p),
        (false, true) => normal(&format!("{cwd}/{p}")),
        _ => None,
    }
}

/// `.` and `..` resolved as text, as bash's logical `cd` does; repeated slashes dropped.
fn normal(p: &str) -> Option<String> {
    let mut parts: Vec<&str> = vec![];
    for c in p.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c),
        }
    }
    Some(format!("/{}", parts.join("/")))
}

fn parent(p: &str) -> Option<String> {
    let (dir, _) = p.rsplit_once('/').filter(|(_, f)| !f.is_empty())?;
    Some(if dir.is_empty() { "/".into() } else { dir.into() })
}

/// One shell word, or `None` for a word whose value needs the shell (a `$`, a glob, a
/// command substitution), and the separators between commands.
#[derive(Debug, PartialEq)]
enum Tok {
    Word(Option<String>),
    Sep,
}

/// The command's words and separators, up to a heredoc (its body is no command) or
/// [`SCAN_MAX`]. Quotes, backslashes, `~` and a leading `$HOME` are the shell's.
fn lex(cmd: &str, home: &str) -> Vec<Tok> {
    let end = cmd.char_indices().nth(SCAN_MAX).map_or(cmd.len(), |(i, _)| i);
    let mut cs = cmd[..end].chars().peekable();
    let (mut out, mut w, mut bad, mut open) = (vec![], String::new(), false, false);
    let flush = |out: &mut Vec<Tok>, w: &mut String, bad: &mut bool, open: &mut bool| {
        if *open {
            let ok = !*bad && w.len() <= WORD_MAX;
            out.push(Tok::Word(ok.then(|| std::mem::take(w))));
        }
        (*bad, *open) = (false, false);
        w.clear();
    };
    // `$HOME` or `${HOME}` at the word's start, and nothing of a longer name after it
    let home_at = |cs: &mut std::iter::Peekable<std::str::Chars>, w: &str| {
        let rest: String = cs.clone().take(7).collect();
        let n = [("{HOME}", 6), ("HOME", 4)].into_iter().find(|(s, _)| rest.starts_with(s))?.1;
        let next = rest.chars().nth(n);
        let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
        (w.is_empty() && !home.is_empty() && (n == 6 || !next.is_some_and(word))).then(|| {
            cs.nth(n - 1);
        })
    };
    while let Some(c) = cs.next() {
        match c {
            ' ' | '\t' => flush(&mut out, &mut w, &mut bad, &mut open),
            '\n' | ';' | '&' | '|' | '(' | ')' | '<' | '>' => {
                flush(&mut out, &mut w, &mut bad, &mut open);
                if c == '<' && cs.peek() == Some(&'<') {
                    return out;
                }
                out.push(Tok::Sep);
            }
            '#' if !open => while cs.next_if(|&c| c != '\n').is_some() {},
            '\'' => {
                open = true;
                loop {
                    match cs.next() {
                        Some('\'') => break,
                        Some(c) => w.push(c),
                        None => return out,
                    }
                }
            }
            '"' => {
                open = true;
                loop {
                    match cs.next() {
                        Some('"') => break,
                        Some('\\') => match cs.next() {
                            Some(e @ ('$' | '`' | '"' | '\\')) => w.push(e),
                            Some('\n') => {}
                            Some(e) => w.extend(['\\', e]),
                            None => return out,
                        },
                        Some('$') => match home_at(&mut cs, &w) {
                            Some(()) => w.push_str(home),
                            None => bad = true,
                        },
                        Some('`') => bad = true,
                        Some(c) => w.push(c),
                        None => return out,
                    }
                }
            }
            '\\' => match cs.next() {
                Some('\n') => {}
                Some(e) => {
                    open = true;
                    w.push(e);
                }
                None => {}
            },
            '$' => {
                match home_at(&mut cs, &w) {
                    Some(()) => w.push_str(home),
                    None => bad = true,
                }
                open = true;
            }
            '~' if !open => {
                open = true;
                match cs.peek() {
                    None | Some('/' | ' ' | '\t' | '\n' | ';' | '&' | '|' | ')') if !home.is_empty() => w.push_str(home),
                    _ => bad = true,
                }
            }
            '`' | '*' | '?' | '[' | '{' => (open, bad) = (true, true),
            c => {
                open = true;
                w.push(c);
            }
        }
    }
    flush(&mut out, &mut w, &mut bad, &mut open);
    out
}

/// The first command of `cmd` that names a folder: `cd`/`pushd` and its argument, or `git`
/// with `-C`. One whose folder needs the shell (`cd "$X"`) ends the read: the rest of the
/// call ran somewhere this cannot name.
fn bash_dir(cmd: &str, cwd: &str, home: &str) -> Option<String> {
    let toks = lex(cmd, home);
    for seg in toks.split(|t| *t == Tok::Sep) {
        let words: Vec<Option<&str>> = seg
            .iter()
            .map(|t| match t {
                Tok::Word(w) => w.as_deref(),
                Tok::Sep => None,
            })
            .collect();
        match words.first() {
            Some(Some("cd" | "pushd")) => {
                let arg = words[1..].iter().find(|w| !w.is_some_and(|w| w.starts_with('-') && w.len() > 1));
                let arg = (*arg?)?;
                return (arg != "-" && !arg.starts_with('+')).then(|| resolve(arg, cwd)).flatten();
            }
            Some(Some("git")) => {
                let (mut at, mut i) = (None::<String>, 1);
                while let Some(w) = words.get(i) {
                    match *w {
                        Some("-C") => {
                            let p = (*words.get(i + 1)?)?;
                            at = Some(resolve(p, at.as_deref().unwrap_or(cwd))?);
                            i += 2;
                        }
                        Some("-c") => i += 2,
                        Some(o) if o.starts_with('-') => i += 1,
                        _ => break,
                    }
                }
                if at.is_some() {
                    return at;
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether `dotgit` makes its folder a work tree's top: a `.git` directory holding `HEAD`,
/// or a `.git` file (a linked worktree, a submodule) whose `gitdir:` names a directory.
fn is_repo(dotgit: &Path) -> bool {
    if dotgit.is_dir() {
        return dotgit.join("HEAD").is_file();
    }
    let Ok(s) = std::fs::read_to_string(dotgit) else { return false };
    let Some(g) = s.lines().next().and_then(|l| l.strip_prefix("gitdir:")) else {
        return false;
    };
    dotgit.parent().is_some_and(|d| d.join(g.trim()).is_dir())
}

/// The git toplevel holding `dir`, symlinks resolved, found by walking up to a `.git`.
/// `dir` itself need not exist (a removed subfolder of a tree that still does).
fn toplevel(dir: &str) -> Option<String> {
    let mut p = Path::new(dir);
    if !p.is_absolute() {
        return None;
    }
    #[cfg(test)]
    WALKS.with(|n| n.set(n.get() + 1));
    loop {
        if is_repo(&p.join(".git")) {
            return std::fs::canonicalize(p).ok().map(|p| p.display().to_string());
        }
        p = p.parent()?;
    }
}

thread_local! {
    /// [`toplevel`]'s answers by folder, each with when it was found.
    static TOPS: RefCell<HashMap<String, (Instant, Option<String>)>> = RefCell::new(HashMap::new());
    #[cfg(test)]
    static WALKS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// [`toplevel`] through the memory, at `now`: an answer younger than [`TTL`] stands.
fn top_at(dir: &str, now: Instant) -> Option<String> {
    let kept = TOPS.with(|c| c.borrow().get(dir).cloned());
    if let Some((_, t)) = kept.filter(|(at, _)| now.saturating_duration_since(*at) < TTL) {
        return t;
    }
    let t = toplevel(dir);
    TOPS.with(|c| {
        let mut c = c.borrow_mut();
        c.insert(dir.to_string(), (now, t.clone()));
        if c.len() > CACHE_MAX {
            c.retain(|_, (at, _)| now.saturating_duration_since(*at) < TTL);
        }
        if c.len() > CACHE_MAX {
            c.clear();
        }
    });
    t
}

/// The folders the fold saw, as the git trees holding them and the seconds since the
/// newest call in each, newest first. A folder in no tree is left out.
pub(super) fn recent(seen: &[(String, SystemTime)], now: SystemTime) -> Vec<Recent> {
    recent_at(seen, now, Instant::now())
}

fn recent_at(seen: &[(String, SystemTime)], now: SystemTime, clock: Instant) -> Vec<Recent> {
    let mut out: Vec<Recent> = vec![];
    for (dir, at) in seen {
        let Some(top) = top_at(dir, clock) else { continue };
        // a stamp from the future is now
        let age = now.duration_since(*at).map_or(0, |d| d.as_secs());
        match out.iter_mut().find(|r| r.dir == top) {
            Some(r) => r.age = r.age.min(age),
            None => out.push(Recent { dir: top, age }),
        }
    }
    out.sort_by_key(|r| r.age);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    const HOME: &str = "/h";

    fn bash(cmd: &str, cwd: &str) -> Option<String> {
        dir_of("Bash", &json!({ "command": cmd }), cwd, HOME)
    }

    /// Rows: [name, command, the folder it names].
    #[test]
    fn a_bash_call_names_the_folder_its_first_cd_or_git_dash_c_reaches() {
        let rows: &[(&str, &str, Option<&str>)] = &[
            ("cd &&", "cd /a/b && make", Some("/a/b")),
            ("cd ;", "cd /a; make", Some("/a")),
            ("cd || exit", "cd /a || exit 1\nmake", Some("/a")),
            ("a bare cd", "cd /a", Some("/a")),
            (
                "after unset and export",
                "unset X Y; export P=\"$HOME/b:$PATH\"; cd /a && ls",
                Some("/a"),
            ),
            ("a subshell", "(cd /a && make)", Some("/a")),
            ("double quotes", "cd \"/a b/c\" && ls", Some("/a b/c")),
            ("single quotes", "cd '/a b' && ls", Some("/a b")),
            ("an escaped space", "cd /a\\ b && ls", Some("/a b")),
            ("~/x", "cd ~/x && ls", Some("/h/x")),
            ("~ alone", "cd ~ && ls", Some("/h")),
            ("$HOME/x", "cd $HOME/x", Some("/h/x")),
            ("\"$HOME\"/x", "cd \"$HOME\"/x", Some("/h/x")),
            ("${HOME}/x", "cd ${HOME}/x", Some("/h/x")),
            ("relative, against the row's cwd", "cd sub && ls", Some("/w/sub")),
            ("dot dot", "cd ../x/./y && ls", Some("/x/y")),
            ("-P skipped", "cd -P /a", Some("/a")),
            ("pushd", "pushd /a >/dev/null", Some("/a")),
            ("git -C", "git -C /a status", Some("/a")),
            ("git -c k=v -C rel", "git -c a.b=c -C rel log", Some("/w/rel")),
            ("git -C a -C b", "git -C /a -C b log", Some("/a/b")),
            ("git --no-pager -C", "git --no-pager -C /a diff", Some("/a")),
            ("plain git, then cd", "git status; cd /a", Some("/a")),
        ];
        for (name, cmd, want) in rows {
            assert_eq!(bash(cmd, "/w").as_deref(), *want, "{name}");
        }
    }

    /// Rows: [name, command]: each names no folder.
    #[test]
    fn a_bash_call_whose_folder_needs_the_shell_names_none() {
        let rows = [
            ("cd alone", "cd && ls"),
            ("cd -", "cd - && ls"),
            ("a command substitution", "cd $(mktemp -d) && ls"),
            ("backticks", "cd `pwd`/x"),
            ("another variable", "cd \"$D\" && git -C /a log"),
            ("$HOMEX is not $HOME", "cd $HOMEX/a"),
            ("a glob", "cd /a* && ls"),
            ("~user", "cd ~bob/x"),
            ("cd as an argument", "echo cd /a"),
            ("a comment", "# cd /a\nls"),
            ("a heredoc body", "cat <<EOF\ncd /a\nEOF"),
            ("plain git", "git status"),
            ("an unclosed quote", "cd '/a"),
            ("empty", ""),
        ];
        for (name, cmd) in rows {
            assert_eq!(bash(cmd, "/w"), None, "{name}");
        }
        assert_eq!(bash("cd sub", ""), None, "relative with no cwd");
        assert_eq!(dir_of("Bash", &json!({ "command": "cd ~/x" }), "/w", ""), None, "~, no HOME");
    }

    /// Rows: [tool, input, the folder it names].
    #[test]
    fn a_file_tool_names_its_files_folder_and_other_tools_none() {
        let rows = [
            ("Edit", json!({"file_path": "/a/b/x.rs"}), Some("/a/b")),
            ("MultiEdit", json!({"file_path": "/a/x.rs"}), Some("/a")),
            ("Write", json!({"file_path": "rel/x.rs"}), Some("/w/rel")),
            ("Read", json!({"file_path": "/x.rs"}), Some("/")),
            ("NotebookEdit", json!({"notebook_path": "/a/n.ipynb"}), Some("/a")),
            ("Read", json!({"file_path": "/"}), None),
            ("Read", json!({"file_path": ""}), None),
            ("Read", json!({"file_path": 3}), None),
            ("Grep", json!({"path": "/a"}), None),
            ("Glob", json!({"path": "/a", "pattern": "*"}), None),
            ("Bash", json!({"command": 7}), None),
            ("Bash", json!("cd /a"), None),
            ("Bash", json!({}), None),
        ];
        for (tool, input, want) in rows {
            assert_eq!(dir_of(tool, &input, "/w", HOME).as_deref(), want, "{tool} {input}");
        }
    }

    /// Removes the tree when dropped.
    struct Tmp(PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A scratch tree: `r` a repo, `r/in` a repo inside it, `w` a linked worktree whose
    /// gitdir is in `r`, `d` a worktree whose gitdir is gone, `e` a `.git` with no HEAD,
    /// `l` a symlink to `r`, `n` no repo.
    fn trees(name: &str) -> (Tmp, String) {
        let t = std::env::temp_dir().join(format!("cockpit-activein-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let mk = |p: &str| std::fs::create_dir_all(t.join(p)).unwrap();
        let put = |p: &str, s: &str| std::fs::write(t.join(p), s).unwrap();
        for d in ["r/.git/worktrees/w", "r/sub", "r/in/.git", "w/src", "d", "e/.git", "n/x"] {
            mk(d);
        }
        put("r/.git/HEAD", "ref: refs/heads/main\n");
        put("r/in/.git/HEAD", "ref: refs/heads/main\n");
        put("w/.git", "gitdir: ../r/.git/worktrees/w\n");
        put("d/.git", "gitdir: /no/such/gitdir\n");
        std::os::unix::fs::symlink(t.join("r"), t.join("l")).unwrap();
        let root = std::fs::canonicalize(&t).unwrap().display().to_string();
        (Tmp(t), root)
    }

    /// Rows: [the folder, its toplevel].
    #[test]
    fn the_toplevel_is_the_nearest_folder_up_with_a_real_dot_git() {
        let (_t, d) = trees("top");
        let rows = [
            ("r", Some("r")),
            ("r/sub", Some("r")),
            ("r/gone/deeper", Some("r")),
            ("r/in", Some("r/in")),
            ("w/src", Some("w")),
            ("d", None),
            ("e", None),
            ("l/sub", Some("r")),
            ("n/x", None),
        ];
        for (dir, want) in rows {
            let want = want.map(|w| format!("{d}/{w}"));
            assert_eq!(toplevel(&format!("{d}/{dir}")), want, "{dir}");
        }
        assert_eq!(toplevel("r/sub"), None, "a relative folder");
    }

    /// Two folders of one tree are one entry at the newer age; a folder in no tree is
    /// left out; a stamp from the future is age 0; newest first.
    #[test]
    fn recent_names_each_tree_once_at_its_newest_age_newest_first() {
        let (_t, d) = trees("recent");
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        let ago = |s: u64| now - Duration::from_secs(s);
        let seen = [
            (format!("{d}/r/sub"), ago(50)),
            (format!("{d}/n/x"), ago(1)),
            (format!("{d}/w/src"), ago(400)),
            (format!("{d}/r"), ago(9)),
            (format!("{d}/r/in"), now + Duration::from_secs(30)),
        ];
        let got = recent_at(&seen, now, Instant::now());
        let want = [("r/in", 0), ("r", 9), ("w", 400)].map(|(t, age)| Recent {
            dir: format!("{d}/{t}"),
            age,
        });
        assert_eq!(got, want);
        assert!(recent_at(&[], now, Instant::now()).is_empty());
    }

    /// One walk per folder per TTL, on a clock the test steps: a kept answer walks
    /// nothing, one past its TTL walks again.
    #[test]
    fn a_folder_is_walked_once_per_ttl() {
        let (_t, d) = trees("cache");
        let (sub, t0) = (format!("{d}/r/sub"), Instant::now());
        let walks = || WALKS.with(std::cell::Cell::get);
        let before = walks();
        let tops = [t0, t0 + TTL - Duration::from_secs(1), t0 + TTL].map(|at| top_at(&sub, at));
        assert_eq!(tops, [0, 1, 2].map(|_| Some(format!("{d}/r"))));
        assert_eq!(walks() - before, 2, "the first look and the one past the TTL");
    }

    /// Past CACHE_MAX folders the memory keeps only what is inside its TTL.
    #[test]
    fn the_memory_drops_stale_folders_past_its_bound() {
        let t0 = Instant::now();
        assert_eq!(top_at("/no/such/old", t0), None);
        (0..CACHE_MAX).for_each(|i| assert_eq!(top_at(&format!("/no/such/{i}"), t0 + TTL), None));
        let (n, old) = TOPS.with(|c| (c.borrow().len(), c.borrow().contains_key("/no/such/old")));
        assert_eq!((n, old), (CACHE_MAX, false));
    }

    /// #1358 review: a `.git` the agent made can be anything. Rows: a FIFO, whose open blocks
    /// until a writer comes, and a symlink to /dev/zero, an endless read. Each is no repo,
    /// and the walk ends at once. Each runs on a thread under a bound, so a red never hangs.
    #[test]
    fn a_dot_git_that_is_no_file_or_dir_is_no_repo_and_never_blocks() {
        let (_t, d) = trees("odd");
        let fifo = std::ffi::CString::new(format!("{d}/n/.git")).unwrap();
        // SAFETY: a NUL-terminated path the test owns
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0, "mkfifo");
        std::fs::create_dir_all(format!("{d}/z/sub")).unwrap();
        std::os::unix::fs::symlink("/dev/zero", format!("{d}/z/.git")).unwrap();
        for row in ["n", "z"] {
            let (tx, rx) = std::sync::mpsc::channel();
            let dir = format!("{d}/{row}/x");
            std::thread::spawn(move || tx.send(toplevel(&dir)));
            assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(None), "{row}");
        }
    }
}

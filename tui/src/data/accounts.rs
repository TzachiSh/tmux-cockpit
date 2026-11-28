//! The owner's Claude Code accounts, `@cockpit_accounts` (#1601): `name=dir` pairs, each dir a
//! `CLAUDE_CONFIG_DIR`. The board names a pane's account from the dir its hooks recorded, and
//! reads transcripts under each account's `projects` too (`transcript::roots`).
use std::path::{Path, PathBuf};

thread_local! {
    /// Read once, when the board starts, as `@cockpit_agents_glob` is.
    static LISTED: Vec<(String, PathBuf)> = listed();
}

/// The option off the server, parsed. A value [`parse`] refuses lists no account, and the
/// log says why.
fn listed() -> Vec<(String, PathBuf)> {
    let v = super::tmux(&["show-option", "-gqv", "@cockpit_accounts"]).unwrap_or_default();
    let home = std::env::var("HOME").unwrap_or_default();
    parse(v.trim(), Path::new(&home)).unwrap_or_else(|e| {
        super::log(&format!("@cockpit_accounts refused: {e}"));
        vec![]
    })
}

/// `p1=~/.claude p2=/srv/c2`, split on whitespace. A name is ASCII letters, digits, `-` and `_`,
/// listed once. A dir is absolute, `~` or under `~/` (`home`), and on disk: it is resolved,
/// symlinks followed, so a pane's dir matches it however either is spelled, and two names on
/// one dir are refused. Any entry that fails refuses the whole value.
pub(crate) fn parse(s: &str, home: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut out: Vec<(String, PathBuf)> = vec![];
    for e in s.split_whitespace() {
        let (name, dir) = e.split_once('=').ok_or_else(|| format!("{e:?} is not name=dir"))?;
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
            return Err(format!("{name:?}: a name is letters, digits, - and _"));
        }
        if out.iter().any(|(n, _)| n == name) {
            return Err(format!("{name:?} is listed twice"));
        }
        let elsewhere = || format!("{dir:?}: a dir is absolute, ~ or under ~/");
        let path = match dir.strip_prefix('~') {
            Some(_) if !home.is_absolute() => return Err(format!("{dir:?}: no HOME for ~")),
            Some("") => home.to_path_buf(),
            Some(rest) => home.join(rest.strip_prefix('/').ok_or_else(elsewhere)?),
            None if dir.starts_with('/') => PathBuf::from(dir),
            None => return Err(elsewhere()),
        };
        let path = std::fs::canonicalize(&path).map_err(|err| format!("{dir:?}: {err}"))?;
        if let Some((n, _)) = out.iter().find(|(_, d)| *d == path) {
            return Err(format!("{n:?} and {name:?} are one dir"));
        }
        out.push((name.to_string(), path));
    }
    Ok(out)
}

/// The name of the listed account whose dir is `cfg`, a pane's `@cockpit_cfg`, resolved; an empty
/// `cfg` is Claude Code's default dir. `?` when `unseen`: the pane's hooks ran under Claude Code's
/// env scrub and saw no dir, so the default is not assumed. `None` when no account is listed
/// or none has that dir.
pub(crate) fn name_of(cfg: &str, unseen: bool) -> Option<String> {
    LISTED.with(|l| {
        if l.is_empty() {
            return None;
        }
        if unseen {
            return Some("?".to_string());
        }
        let dir = if cfg.is_empty() {
            super::default_config_dir()
        } else {
            PathBuf::from(cfg)
        };
        let dir = std::fs::canonicalize(dir).ok()?;
        l.iter().find(|(_, d)| *d == dir).map(|(n, _)| n.clone())
    })
}

/// Each listed account's `projects` dir, in the option's order.
pub(crate) fn projects() -> Vec<PathBuf> {
    LISTED.with(|l| l.iter().map(|(_, d)| d.join("projects")).collect())
}

#[cfg(test)]
mod tests {
    use super::parse;
    use std::path::{Path, PathBuf};

    /// A home with `.claude`, `.claude-2` and a link `c2` to the second, removed on drop.
    struct Home(PathBuf);
    impl Drop for Home {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn home(tag: &str) -> Home {
        let h = std::env::temp_dir().join(format!("cockpit-accounts-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&h);
        std::fs::create_dir_all(h.join(".claude")).unwrap();
        std::fs::create_dir_all(h.join(".claude-2")).unwrap();
        std::os::unix::fs::symlink(h.join(".claude-2"), h.join("c2")).unwrap();
        Home(std::fs::canonicalize(&h).unwrap())
    }
    fn names(v: &[(String, PathBuf)]) -> Vec<(&str, &Path)> {
        v.iter().map(|(n, d)| (n.as_str(), d.as_path())).collect()
    }

    #[test]
    fn a_list_reads_tilde_and_absolute_dirs_resolved() {
        let h = home("ok");
        let abs = format!("p2={}/c2/", h.0.display());
        let v = parse(&format!("p1=~/.claude  {abs}\twork=~"), &h.0).unwrap();
        let want = [("p1", h.0.join(".claude")), ("p2", h.0.join(".claude-2")), ("work", h.0.clone())];
        assert_eq!(names(&v), want.iter().map(|(n, d)| (*n, d.as_path())).collect::<Vec<_>>());
        assert!(parse("", &h.0).unwrap().is_empty(), "unset lists none");
    }

    #[test]
    fn any_bad_entry_refuses_the_whole_list() {
        let h = home("bad");
        let bad = [
            ("p1=~/.claude p2", "not name=dir"),
            ("p1=~/.claude =~/.claude-2", "a name is"),
            ("p.1=~/.claude", "a name is"),
            ("p1=~/.claude p1=~/.claude-2", "listed twice"),
            ("p1=.claude", "a dir is absolute"),
            ("p1=~x/.claude", "a dir is absolute"),
            ("p1=~/.claude-9", "No such file"),
            ("p1=~/.claude-2 p2=~/c2", "are one dir"),
        ];
        for (s, why) in bad {
            let e = parse(s, &h.0).expect_err(s);
            assert!(e.contains(why), "{s}: {e}");
        }
        assert!(parse("p1=~/.claude", Path::new("")).unwrap_err().contains("no HOME"));
    }
}

//! The owner's auto-answer rules (#1508, epic #1500): the fixed action set, the closed
//! predicate list, and the rules file, loaded and checked so that any doubt turns a rule
//! off. docs/ask.md "The rules file" is the freeze. The cockpit never writes the rules
//! file; `accept` writes only the accepted copy, on the owner's yes.
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

/// The largest rules file or accepted copy read, in bytes.
pub const FILE_MAX: u64 = 64 * 1024;
/// The most lines either file may hold, which bounds the line diff.
pub const LINES_MAX: usize = 2000;
/// D6: a rule with no `cap_per_hour` answers at most this often.
pub const CAP_DEFAULT: u32 = 3;
/// The largest `cap_per_hour` a rule may set.
pub const CAP_MAX: u32 = 60;

const TOP_KEYS: &[&str] = &["schema_version", "rules"];
const RULE_KEYS: &[&str] = &["id", "note", "action", "when", "mode", "cap_per_hour", "examples"];
const EXAMPLE_KEYS: &[&str] = &["match", "no_match"];

/// The one fixed action set: what a rule may answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    Merge,
    CleanupLane,
    ContinueQueue,
}

impl ActionKind {
    pub fn from_name(s: &str) -> Option<ActionKind> {
        match s {
            "merge" => Some(ActionKind::Merge),
            "cleanup-lane" => Some(ActionKind::CleanupLane),
            "continue-queue" => Some(ActionKind::ContinueQueue),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ActionKind::Merge => "merge",
            ActionKind::CleanupLane => "cleanup-lane",
            ActionKind::ContinueQueue => "continue-queue",
        }
    }
}

/// The closed predicate list. `ask::facts` (#1509) computes each one; none reads
/// question text (R6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pred {
    VerdictMergeAtHead,
    CiGreenOnMerge,
    PrMerged,
    LaneClean,
    QueueSet,
}

impl Pred {
    pub fn from_name(s: &str) -> Option<Pred> {
        match s {
            "verdict_merge_at_head" => Some(Pred::VerdictMergeAtHead),
            "ci_green_on_merge" => Some(Pred::CiGreenOnMerge),
            "pr_merged" => Some(Pred::PrMerged),
            "lane_clean" => Some(Pred::LaneClean),
            "queue_set" => Some(Pred::QueueSet),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Pred::VerdictMergeAtHead => "verdict_merge_at_head",
            Pred::CiGreenOnMerge => "ci_green_on_merge",
            Pred::PrMerged => "pr_merged",
            Pred::LaneClean => "lane_clean",
            Pred::QueueSet => "queue_set",
        }
    }
}

/// A fact set. An absent predicate is unknown and fails every rule that needs it.
pub type Facts = BTreeMap<Pred, bool>;

/// The fact sets a rule must match, and those it must not.
#[derive(Clone, Debug, PartialEq)]
pub struct Examples {
    pub matching: Vec<Facts>,
    pub not_matching: Vec<Facts>,
}

/// Every rule starts in `Shadow` (R7); `Live` is the owner's own edit, then `accept`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Shadow,
    Live,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Shadow => "shadow",
            Mode::Live => "live",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    pub id: String,
    pub action: ActionKind,
    pub when: Vec<Pred>,
    pub mode: Mode,
    pub cap_per_hour: u32,
    pub examples: Examples,
}

impl Rule {
    /// Every predicate the rule names is true in `f`; an absent one fails.
    pub fn matches(&self, f: &Facts) -> bool {
        self.when.iter().all(|p| f.get(p) == Some(&true))
    }
}

/// Why a rule, or the whole file, is off.
#[derive(Clone, Debug, PartialEq)]
pub enum Off {
    /// The file, by name, is not there.
    Missing(&'static str),
    /// The file, by name, is refused: a symlink, not a regular file, not the owner's,
    /// not mode 0600, too large or too long.
    Refused(&'static str, &'static str),
    Unreadable(&'static str, String),
    Parse(String),
    UnknownKey(String),
    NoAcceptedCopy,
    NotAccepted,
    BadField(&'static str),
    DupId,
    UnknownPred(String),
    BadCap,
    ExampleFails {
        kind: &'static str,
        index: usize,
    },
}

impl fmt::Display for Off {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Off::Missing(w) => write!(f, "{w} is missing"),
            Off::Refused(w, why) => write!(f, "{w} is {why}"),
            Off::Unreadable(w, e) => write!(f, "{w} cannot be read: {e}"),
            Off::Parse(e) => write!(f, "not a rules file: {e}"),
            Off::UnknownKey(k) => write!(f, "unknown key {k}"),
            Off::NoAcceptedCopy => write!(f, "never accepted: run cockpit-tui ask-rules accept"),
            Off::NotAccepted => write!(f, "changed since it was accepted: run cockpit-tui ask-rules accept"),
            Off::BadField(k) => write!(f, "bad {k}"),
            Off::DupId => write!(f, "duplicate id"),
            Off::UnknownPred(n) => write!(f, "unknown predicate {n}"),
            Off::BadCap => write!(f, "cap_per_hour must be 1 to {CAP_MAX}"),
            Off::ExampleFails { kind, index } => write!(f, "{kind} example {index} fails"),
        }
    }
}

/// A loaded file. While `file` is an error, every rule is off, however its own
/// check went; `rules` names each rule by id, or by `rules[<i>]` without a good one.
#[derive(Debug, PartialEq)]
pub struct Rules {
    pub file: Result<(), Off>,
    pub rules: Vec<(String, Result<Rule, Off>)>,
}

impl Rules {
    /// The rules that are on: none while the file is off.
    pub fn on(&self) -> Vec<&Rule> {
        if self.file.is_err() {
            return Vec::new();
        }
        self.rules.iter().filter_map(|(_, r)| r.as_ref().ok()).collect()
    }
}

/// Why `accept` wrote nothing.
#[derive(Debug)]
pub enum Error {
    Invalid(Off),
    RuleOff(String, Off),
    Declined,
    Io(PathBuf, io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Invalid(o) => write!(f, "{o}"),
            Error::RuleOff(id, o) => write!(f, "rule {id} is off ({o}), so nothing was accepted"),
            Error::Declined => write!(f, "not accepted"),
            Error::Io(p, e) => write!(f, "{}: {e}", p.display()),
        }
    }
}

/// The accepted copy against the file, line by line: ' ' kept, '-' only in the
/// copy, '+' only in the file.
#[derive(Debug, PartialEq)]
pub struct Diff(pub Vec<(char, String)>);

impl Diff {
    /// Whether any line differs.
    pub fn changed(&self) -> bool {
        self.0.iter().any(|(c, _)| *c != ' ')
    }
}

impl fmt::Display for Diff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (c, line) in &self.0 {
            writeln!(f, "{c}{line}")?;
        }
        Ok(())
    }
}

/// The rules file and the accepted copy, by docs/ask.md: under XDG_CONFIG_HOME and
/// XDG_STATE_HOME, by [`crate::store::xdg_dir`].
pub fn paths_from(config_home: Option<&str>, state_home: Option<&str>, home: Option<&str>) -> Option<(PathBuf, PathBuf)> {
    use crate::store::xdg_dir;
    let rules = xdg_dir(config_home, home, ".config")?.join("tmux-cockpit/ask-rules.json");
    let accepted = xdg_dir(state_home, home, ".local/state")?.join("tmux-cockpit/ask-rules.accepted.json");
    Some((rules, accepted))
}

/// The rules file, checked whole, then held against the accepted copy. Fails closed:
/// any doubt is an [`Off`], for the file or for one rule.
pub fn load(rules: &Path, accepted: &Path) -> Rules {
    let bytes = match read_owned(rules, "the rules file") {
        Ok(b) => b,
        Err(o) => {
            return Rules {
                file: Err(o),
                rules: Vec::new(),
            };
        }
    };
    let mut loaded = parse(&bytes);
    if loaded.file.is_ok() {
        loaded.file = held(&bytes, accepted);
    }
    loaded
}

/// Ok only when the accepted copy holds exactly these bytes.
fn held(bytes: &[u8], accepted: &Path) -> Result<(), Off> {
    match read_owned(accepted, "the accepted copy") {
        Ok(copy) if copy == bytes => Ok(()),
        Ok(_) => Err(Off::NotAccepted),
        Err(Off::Missing(_)) => Err(Off::NoAcceptedCopy),
        Err(o) => Err(o),
    }
}

/// Reads the rules file once, checks it as [`load`] does with every rule on, shows
/// `yes` the diff against the accepted copy, and on yes makes exactly those bytes the
/// copy. An unchanged file asks nothing.
pub fn accept(rules: &Path, accepted: &Path, yes: impl FnOnce(&Diff) -> bool) -> Result<Diff, Error> {
    let bytes = read_owned(rules, "the rules file").map_err(Error::Invalid)?;
    let parsed = parse(&bytes);
    if let Err(o) = parsed.file {
        return Err(Error::Invalid(o));
    }
    if let Some((id, Err(o))) = parsed.rules.into_iter().find(|(_, r)| r.is_err()) {
        return Err(Error::RuleOff(id, o));
    }
    let old = match read_owned(accepted, "the accepted copy") {
        Ok(c) => c,
        Err(Off::Missing(_)) => Vec::new(),
        Err(o) => return Err(Error::Invalid(o)),
    };
    let d = diff(&String::from_utf8_lossy(&old), &String::from_utf8_lossy(&bytes));
    if old == bytes {
        return Ok(d);
    }
    if !yes(&d) {
        return Err(Error::Declined);
    }
    let dir = accepted
        .parent()
        .ok_or_else(|| Error::Io(accepted.to_path_buf(), io::Error::other("no folder")))?;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|e| Error::Io(dir.to_path_buf(), e))?;
    let mut tmp = accepted.as_os_str().to_owned();
    tmp.push("~tmp");
    crate::store::replace_file(dir, Path::new(&tmp), accepted, &bytes).map_err(|(p, e)| Error::Io(p, e))?;
    Ok(d)
}

/// The bytes of a regular file the owner holds at mode 0600, opened without following
/// a symlink, and checked on the open file itself.
fn read_owned(p: &Path, what: &'static str) -> Result<Vec<u8>, Off> {
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(p);
    let f = match f {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(Off::Missing(what)),
        Err(e) if e.raw_os_error() == Some(libc::ELOOP) => return Err(Off::Refused(what, "a symlink")),
        Err(e) => return Err(Off::Unreadable(what, e.to_string())),
    };
    let m = f.metadata().map_err(|e| Off::Unreadable(what, e.to_string()))?;
    // SAFETY: geteuid takes no argument and cannot fail.
    let euid = unsafe { libc::geteuid() };
    perms(m.is_file(), m.uid(), m.mode(), m.len(), euid).map_err(|why| Off::Refused(what, why))?;
    let mut bytes = Vec::new();
    f.take(FILE_MAX + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| Off::Unreadable(what, e.to_string()))?;
    if bytes.len() as u64 > FILE_MAX {
        return Err(Off::Refused(what, "larger than 64 KiB"));
    }
    Ok(bytes)
}

/// The owner-only rule, on plain values so a row can try another uid.
fn perms(is_file: bool, uid: u32, mode: u32, len: u64, euid: u32) -> Result<(), &'static str> {
    if !is_file {
        return Err("not a regular file");
    }
    if uid != euid {
        return Err("not the owner's");
    }
    if mode & 0o777 != 0o600 {
        return Err("not mode 0600");
    }
    if len > FILE_MAX {
        return Err("larger than 64 KiB");
    }
    Ok(())
}

/// The file's text as rules. A shape or key error refuses the whole file; a bad rule is
/// off alone.
fn parse(bytes: &[u8]) -> Rules {
    let refuse = |o: Off| Rules {
        file: Err(o),
        rules: Vec::new(),
    };
    let Ok(text) = std::str::from_utf8(bytes) else {
        return refuse(Off::Parse("not UTF-8".into()));
    };
    if text.lines().count() > LINES_MAX {
        return refuse(Off::Refused("the rules file", "longer than 2000 lines"));
    }
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => return refuse(Off::Parse(e.to_string())),
    };
    let Some(top) = v.as_object() else {
        return refuse(Off::Parse("not a JSON object".into()));
    };
    if let Some(k) = unknown(top, TOP_KEYS, "") {
        return refuse(Off::UnknownKey(k));
    }
    if top.get("schema_version") != Some(&Value::from(1)) {
        return refuse(Off::Parse("schema_version is not 1".into()));
    }
    let Some(list) = top.get("rules").and_then(Value::as_array) else {
        return refuse(Off::Parse("rules is not a list".into()));
    };
    let mut objs = Vec::new();
    for (i, r) in list.iter().enumerate() {
        let Some(o) = r.as_object() else {
            return refuse(Off::Parse(format!("rules[{i}] is not an object")));
        };
        if let Some(k) = unknown(o, RULE_KEYS, &format!("rules[{i}].")) {
            return refuse(Off::UnknownKey(k));
        }
        if let Some(ex) = o.get("examples").and_then(Value::as_object)
            && let Some(k) = unknown(ex, EXAMPLE_KEYS, &format!("rules[{i}].examples."))
        {
            return refuse(Off::UnknownKey(k));
        }
        objs.push(o);
    }
    let mut seen = HashSet::new();
    let rules = objs
        .into_iter()
        .enumerate()
        .map(|(i, o)| {
            let name = o.get("id").and_then(Value::as_str).filter(|s| id_ok(s));
            let name = name.map_or_else(|| format!("rules[{i}]"), str::to_string);
            (name, read_rule(o, &mut seen))
        })
        .collect();
    Rules { file: Ok(()), rules }
}

/// The first key of `o` not in `allowed`, as `<at><key>`.
fn unknown(o: &Map<String, Value>, allowed: &[&str], at: &str) -> Option<String> {
    o.keys().find(|k| !allowed.contains(&k.as_str())).map(|k| format!("{at}{k}"))
}

/// `[a-z][a-z0-9-]{0,31}`
fn id_ok(s: &str) -> bool {
    let b = s.as_bytes();
    (1..=32).contains(&b.len()) && b[0].is_ascii_lowercase() && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

fn read_rule(o: &Map<String, Value>, seen: &mut HashSet<String>) -> Result<Rule, Off> {
    let id = o
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| id_ok(s))
        .ok_or(Off::BadField("id"))?;
    if !seen.insert(id.to_string()) {
        return Err(Off::DupId);
    }
    if o.get("note").is_some_and(|n| !n.is_string()) {
        return Err(Off::BadField("note"));
    }
    let action = o
        .get("action")
        .and_then(Value::as_str)
        .and_then(ActionKind::from_name)
        .ok_or(Off::BadField("action"))?;
    let when = preds(o.get("when"))?;
    let mode = match o.get("mode").and_then(Value::as_str) {
        Some("shadow") => Mode::Shadow,
        Some("live") => Mode::Live,
        _ => return Err(Off::BadField("mode")),
    };
    let cap_per_hour = match o.get("cap_per_hour") {
        None => CAP_DEFAULT,
        Some(v) => v.as_u64().filter(|c| (1..=u64::from(CAP_MAX)).contains(c)).ok_or(Off::BadCap)? as u32,
    };
    let ex = o.get("examples").and_then(Value::as_object).ok_or(Off::BadField("examples"))?;
    let examples = Examples {
        matching: fact_sets(ex.get("match"))?,
        not_matching: fact_sets(ex.get("no_match"))?,
    };
    let rule = Rule {
        id: id.to_string(),
        action,
        when,
        mode,
        cap_per_hour,
        examples,
    };
    for (index, f) in rule.examples.matching.iter().enumerate() {
        if !rule.matches(f) {
            return Err(Off::ExampleFails { kind: "match", index });
        }
    }
    for (index, f) in rule.examples.not_matching.iter().enumerate() {
        if rule.matches(f) {
            return Err(Off::ExampleFails { kind: "no_match", index });
        }
    }
    Ok(rule)
}

/// A non-empty list of known predicate names: a rule that names none would match
/// every fact set.
fn preds(v: Option<&Value>) -> Result<Vec<Pred>, Off> {
    let list = v.and_then(Value::as_array).filter(|l| !l.is_empty()).ok_or(Off::BadField("when"))?;
    let mut out = Vec::new();
    for n in list {
        let n = n.as_str().ok_or(Off::BadField("when"))?;
        match Pred::from_name(n) {
            Some(p) => out.push(p),
            None => return Err(Off::UnknownPred(n.to_string())),
        }
    }
    Ok(out)
}

/// A non-empty list of fact sets, each an object of known predicate names to booleans.
fn fact_sets(v: Option<&Value>) -> Result<Vec<Facts>, Off> {
    let list = v
        .and_then(Value::as_array)
        .filter(|l| !l.is_empty())
        .ok_or(Off::BadField("examples"))?;
    list.iter()
        .map(|f| {
            let o = f.as_object().ok_or(Off::BadField("examples"))?;
            o.iter()
                .map(|(k, b)| {
                    let p = Pred::from_name(k).ok_or_else(|| Off::UnknownPred(k.clone()))?;
                    Ok((p, b.as_bool().ok_or(Off::BadField("examples"))?))
                })
                .collect()
        })
        .collect()
}

/// A line diff by longest common subsequence; [`LINES_MAX`] bounds the table.
fn diff(old: &str, new: &str) -> Diff {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let (n, m) = (a.len(), b.len());
    let mut l = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            l[i][j] = if a[i] == b[j] {
                l[i + 1][j + 1] + 1
            } else {
                l[i + 1][j].max(l[i][j + 1])
            };
        }
    }
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < n && j < m {
        if a[i] == b[j] {
            out.push((' ', a[i].to_string()));
            i += 1;
            j += 1;
        } else if l[i + 1][j] >= l[i][j + 1] {
            out.push(('-', a[i].to_string()));
            i += 1;
        } else {
            out.push(('+', b[j].to_string()));
            j += 1;
        }
    }
    out.extend(a[i..].iter().map(|s| ('-', s.to_string())));
    out.extend(b[j..].iter().map(|s| ('+', s.to_string())));
    Diff(out)
}

/// One line per rule: `<id> on <mode> <action> when <preds> cap <n>/h` or `<id> off: <why>`.
fn report(r: &Rules) -> String {
    let mut s = String::new();
    if let Err(o) = &r.file {
        s.push_str(&format!("off: {o}\n"));
    }
    for (id, rule) in &r.rules {
        match (rule, &r.file) {
            (Ok(rule), Ok(())) => {
                let when: Vec<&str> = rule.when.iter().map(|p| p.name()).collect();
                s.push_str(&format!(
                    "{id} on {} {} when {} cap {}/h\n",
                    rule.mode.name(),
                    rule.action.name(),
                    when.join(","),
                    rule.cap_per_hour
                ));
            }
            (Ok(_), Err(_)) => s.push_str(&format!("{id} off: the file is off\n")),
            (Err(o), _) => s.push_str(&format!("{id} off: {o}\n")),
        }
    }
    s
}

const USAGE: &str = "usage: cockpit-tui ask-rules check|accept";

/// `cockpit-tui ask-rules check|accept`. check: 0 when every rule is on, 1 when anything is
/// off. accept: 0 when the copy holds the file, 1 on a refusal or a no. 2 on usage.
pub fn main(args: &[String]) -> i32 {
    let var = |k| std::env::var(k).ok();
    let Some((rules, accepted)) = paths_from(
        var("XDG_CONFIG_HOME").as_deref(),
        var("XDG_STATE_HOME").as_deref(),
        var("HOME").as_deref(),
    ) else {
        eprintln!("cockpit-tui: ask-rules: no HOME and no absolute XDG folders");
        return 1;
    };
    match args {
        [a] if a == "check" => {
            let r = load(&rules, &accepted);
            print!("{}", report(&r));
            if r.file.is_ok() && r.on().len() == r.rules.len() { 0 } else { 1 }
        }
        [a] if a == "accept" => {
            let asked = accept(&rules, &accepted, |d| {
                print!("{d}accept these rules? [y/N] ");
                let _ = io::stdout().flush();
                let mut line = String::new();
                io::stdin().read_line(&mut line).is_ok() && matches!(line.trim(), "y" | "yes")
            });
            match asked {
                Ok(d) if d.changed() => {
                    println!("accepted: {}", accepted.display());
                    0
                }
                Ok(_) => {
                    println!("already accepted");
                    0
                }
                Err(e) => {
                    eprintln!("cockpit-tui: ask-rules: {e}");
                    1
                }
            }
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #1508: a file that is not the owner's is refused, with every other part right.
    #[test]
    fn another_owners_file_is_refused() {
        assert_eq!(perms(true, 1000, 0o100600, 10, 1000), Ok(()));
        assert_eq!(perms(true, 1001, 0o100600, 10, 1000), Err("not the owner's"));
    }

    /// #1508: one line changed between the copy and the file shows as one removed and
    /// one added, with the kept lines around them.
    #[test]
    fn the_diff_marks_removed_and_added_lines() {
        let d = diff("a\nb\nc\n", "a\nx\nc\n");
        let want: Vec<(char, String)> = vec![(' ', "a".into()), ('-', "b".into()), ('+', "x".into()), (' ', "c".into())];
        assert_eq!(d.0, want);
        assert!(d.changed());
        assert!(!diff("a\n", "a\n").changed());
    }
}

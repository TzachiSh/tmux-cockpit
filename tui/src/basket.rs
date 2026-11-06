//! The prompt basket (#365 child 5): code references gathered for one pane's
//! agent. It lives in the pane option `@cockpit_basket`, so it goes with the
//! pane, as one `refs::render` line per ref, each an absolute path.
//!
//! The viewer and the browser add to it; the panel shows it, removes from it,
//! and sends it with a draft. Every write goes through `data::tmux_set`, the
//! one write path `COCKPIT_READONLY` turns off, and is read back to be sure.
use crate::panel::is_pane;
use crate::refs::{self, Ref};
use std::path::Path;

pub const OPTION: &str = "@cockpit_basket";
pub const MAX_REFS: usize = 32;
/// tmux takes one command of about 16 KB (16,343 bytes probed on tmux 3.8;
/// a 20,000-byte `set-option` failed "command too long"): the text stays under.
pub const MAX_BYTES: usize = 12 * 1024;

/// One ref's line: `refs::render` from `/` leaves the leading slash off, so it is
/// put back, and the line reads as the absolute path it is.
fn line(r: &Ref) -> String {
    format!("/{}", refs::render(std::slice::from_ref(r), Path::new("/")))
}

/// The option's text: one line per ref, each an absolute path. Over
/// [`MAX_BYTES`] is an error, never a cut, as a cut basket would drop refs.
pub fn body(refs: &[Ref]) -> Result<String, String> {
    let text = refs.iter().map(line).collect::<Vec<_>>().join("\n");
    if text.len() > MAX_BYTES {
        let n = text.len();
        return Err(format!("the basket would be too long ({n} bytes, {MAX_BYTES} at most)"));
    }
    Ok(text)
}

/// The refs the option's text holds, each once; a file gone since is dropped.
pub fn from_body(text: &str) -> Vec<Ref> {
    let mut out: Vec<Ref> = vec![];
    for r in refs::parse(text, Path::new("/")) {
        if !out.contains(&r) {
            out.push(r);
        }
    }
    out
}

/// `refs` with `r` added at the end, and whether it was new. A ref the text
/// would not read back as itself (a line break or control character in its
/// name) is refused, as are an add past [`MAX_REFS`] or [`MAX_BYTES`].
pub fn with_ref(refs: &[Ref], r: Ref) -> Result<(Vec<Ref>, bool), String> {
    if refs.contains(&r) {
        return Ok((refs.to_vec(), false));
    }
    if from_body(&line(&r)) != [r.clone()] {
        return Err("its name cannot be kept as text; not added".into());
    }
    if refs.len() >= MAX_REFS {
        return Err(format!("the basket holds {MAX_REFS} refs at most; remove one first"));
    }
    let mut out = refs.to_vec();
    out.push(r);
    body(&out)?;
    Ok((out, true))
}

fn check(pane: &str) -> Result<(), String> {
    if is_pane(pane) {
        Ok(())
    } else {
        Err(format!("{pane:?} is no pane id"))
    }
}

/// Under test the basket runs tmux only on a private server (`-L`, `-S`) or a
/// stub, never the user's: a test that slips past its own setup refuses
/// instead of writing. On in any debug build, so whatever cwd `cargo test` runs
/// from (cargo reads `.cargo/config.toml` from the cwd up: review-510), and
/// wherever `COCKPIT_TEST_TMUX_GUARD` is set (the repo root's config sets it,
/// for a release test run); `COCKPIT_TEST_TMUX_GUARD=0` lifts it by hand.
fn guarded() -> bool {
    match std::env::var("COCKPIT_TEST_TMUX_GUARD").as_deref() {
        Ok("0") => false,
        Ok(_) => true,
        Err(_) => cfg!(debug_assertions),
    }
}

fn guard() -> Result<(), String> {
    if !guarded() {
        return Ok(());
    }
    let argv = crate::data::tmux_argv();
    let stub = argv.first().is_some_and(|p| Path::new(p).file_name().is_some_and(|n| n != "tmux"));
    if stub || argv.iter().any(|a| a == "-L" || a == "-S") {
        Ok(())
    } else {
        Err("COCKPIT_TEST_TMUX_GUARD: no private tmux in COCKPIT_TMUX; refused".into())
    }
}

/// A tmux read, behind [`guard`].
fn ask(args: &[&str]) -> Result<Option<String>, String> {
    guard()?;
    Ok(crate::data::tmux(args))
}

/// The basket of `pane`, read from tmux. A pane that is gone is an error, not
/// an empty basket: tmux reads a gone pane's option as nothing, exit 0.
pub fn read(pane: &str) -> Result<Vec<Ref>, String> {
    check(pane)?;
    let id = ask(&["display-message", "-p", "-t", pane, "#{pane_id}"])?;
    if id.as_deref().map(str::trim) != Some(pane) {
        return Err(format!("the pane {pane} is gone"));
    }
    let show = ["show-options", "-pqv", "-t", pane, OPTION];
    let text = ask(&show)?.ok_or("tmux did not answer")?;
    Ok(from_body(text.trim_end_matches('\n')))
}

/// Sets the basket of `pane` to `refs` (unsets it when empty), then reads it
/// back: a write that did not land is an error, `COCKPIT_READONLY` included.
pub fn write(pane: &str, refs: &[Ref]) -> Result<(), String> {
    check(pane)?;
    let text = body(refs)?;
    if crate::data::readonly() {
        return Err("COCKPIT_READONLY is set; the basket is unchanged".into());
    }
    guard()?;
    if text.is_empty() {
        crate::data::tmux_set(&["set-option", "-pu", "-t", pane, OPTION]);
    } else {
        // a closing newline: tmux reads a trailing `;` on the last argument as a
        // command separator and strips it (review-534); `read` trims the newline
        crate::data::tmux_set(&["set-option", "-p", "-t", pane, OPTION, &format!("{text}\n")]);
    }
    if read(pane)? != refs {
        return Err("tmux did not keep the basket".into());
    }
    Ok(())
}

/// Adds `r` to the basket of `pane`: how many it holds now, and whether `r` was new.
pub fn add(pane: &str, r: Ref) -> Result<(usize, bool), String> {
    check(pane)?;
    let (refs, added) = with_ref(&read(pane)?, r)?;
    if added {
        write(pane, &refs)?;
    }
    Ok((refs.len(), added))
}

/// `r` as the pane's agent would read it: from the pane's folder when inside
/// it; absolute with no pane, or outside its folder.
pub fn shown(pane: Option<&str>, r: &Ref) -> String {
    let folder = |p: &str| {
        let at = ["display-message", "-p", "-t", p, "#{pane_current_path}"];
        ask(&at).ok().flatten()
    };
    let dir = pane.filter(|p| is_pane(p)).and_then(folder);
    let dir = dir.map_or_else(String::new, |d| d.trim_end_matches('\n').to_string());
    if dir.is_empty() {
        line(r)
    } else {
        refs::render(std::slice::from_ref(r), Path::new(&dir))
    }
}

const USAGE: &str = "usage: cockpit-tui basket add <pane> <file>";

/// `cockpit-tui basket add <pane> <file>`: the browser's alt-a, as
/// tmux-open-path routes it. The exit code (0 added or there already, 1
/// refused, 2 usage) and the line to show; the line names the ref, never
/// the basket's other contents.
pub fn cli(args: &[String]) -> (i32, String) {
    let [verb, pane, file] = args else { return (2, USAGE.into()) };
    if verb != "add" {
        return (2, USAGE.into());
    }
    let r = match Ref::new(file, None, None) {
        Ok(r) if r.path().is_file() => r,
        Ok(_) => return (1, "not a file; only a file goes in the basket".into()),
        Err(e) => return (1, e.to_string()),
    };
    let shown = shown(Some(pane), &r);
    match add(pane, r) {
        Ok((n, true)) => (0, format!("added {shown} ({n} in the basket)")),
        Ok((n, false)) => (0, format!("{shown} is in the basket already ({n})")),
        Err(e) => (1, e),
    }
}

/// [`cli`], printed: the line on stdout, for the launcher to show.
pub fn main(args: &[String]) -> i32 {
    let (code, line) = cli(args);
    println!("{line}");
    code
}

/// A command that is Claude Code's: `claude`, or its version-shaped process
/// title (`2.1.318`), as data's agentish reads them; the one agent that inlines
/// `@path#La-b`.
pub fn claude_cmd(comm: &str) -> bool {
    let b = comm.rsplit('/').next().unwrap_or(comm).trim_start_matches('-').to_ascii_lowercase();
    let version = b.starts_with(|c: char| c.is_ascii_digit())
        && b.chars().filter(|c| *c == '.' || *c == '_').count() >= 2
        && b.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '_');
    b == "claude" || version
}

/// An agent's pane (#519's `data::is_agent`) that runs Claude Code
/// ([`claude_cmd`]). @panestate alone is not enough: it outlives the claude
/// that set it, and @ refs sent to a shell would be noise. A miss sends bare refs.
pub fn is_claude(pane: &str) -> bool {
    if check(pane).is_err() || !crate::data::is_agent(pane) {
        return false;
    }
    let cmd = ask(&["display-message", "-p", "-t", pane, "#{pane_current_command}"]);
    cmd.ok().flatten().is_some_and(|c| claude_cmd(c.trim_end_matches('\n')))
}

/// The send's cap (#365 child 5): the whole text, the draft and its refs. Over it
/// the send is refused, never cut, as a cut text would drop refs the user saw.
pub const MAX_SEND: usize = 16 * 1024;

/// One ref as the pane's agent reads it (owner's D4). For Claude Code, which
/// inlines the lines a mention names (probed on 2.1.318): `@path#La-b`,
/// `@path#La`, `@path` for a whole file; a path with whitespace quoted whole,
/// `@"path#La-b"`, the one quoted form that keeps the range. A path holding `"`
/// cannot be quoted, and goes bare. Any other agent gets `refs::render`'s bare
/// `path:a-b`. Relative to `base`, the pane's folder, when inside it.
pub fn ref_line(r: &Ref, base: &Path, claude: bool) -> String {
    let bare = refs::render(std::slice::from_ref(r), base);
    if !claude {
        return bare;
    }
    let Ok(whole) = Ref::new(r.path(), None, None) else { return bare };
    let path = refs::render(std::slice::from_ref(&whole), base);
    let at = match r.lines() {
        Some((a, b)) if a == b => format!("#L{a}"),
        Some((a, b)) => format!("#L{a}-{b}"),
        None => String::new(),
    };
    if path.contains('"') {
        bare
    } else if path.contains(char::is_whitespace) {
        format!("@\"{path}{at}\"")
    } else {
        format!("@{path}{at}")
    }
}

/// What a send with `refs` puts in the pane: the draft, a blank line, then one
/// ref per line ([`ref_line`]); with none, the draft as it is. This exact text is
/// what the confirm shows. Over [`MAX_SEND`] is an error.
pub fn expand(draft: &str, refs: &[Ref], base: &Path, claude: bool) -> Result<String, String> {
    let text = if refs.is_empty() {
        draft.to_string()
    } else {
        let lines: Vec<String> = refs.iter().map(|r| ref_line(r, base, claude)).collect();
        format!("{}\n\n{}", draft.trim_end_matches('\n'), lines.join("\n"))
    };
    if text.len() > MAX_SEND {
        let n = text.len();
        return Err(format!(
            "too long to send ({n} bytes, {MAX_SEND} at most): remove refs or shorten the draft"
        ));
    }
    Ok(text)
}

//! The prompt basket (#365 child 5): code references gathered for one pane's
//! agent. It lives in the pane option `@cockpit_basket`, so it goes with the
//! pane, as one `refs::render` line per ref, paths taken from `/`.
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

/// The option's text: one line per ref, paths from `/`. Over [`MAX_BYTES`] is
/// an error, never a cut, as a cut basket would drop refs the user added.
pub fn body(refs: &[Ref]) -> Result<String, String> {
    let text = refs::render(refs, Path::new("/"));
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
    if from_body(&refs::render(std::slice::from_ref(&r), Path::new("/"))) != [r.clone()] {
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

/// The basket of `pane`, read from tmux.
pub fn read(pane: &str) -> Result<Vec<Ref>, String> {
    check(pane)?;
    let show = ["show-options", "-pqv", "-t", pane, OPTION];
    let text = crate::data::tmux(&show).ok_or("tmux did not answer")?;
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
    if text.is_empty() {
        crate::data::tmux_set(&["set-option", "-pu", "-t", pane, OPTION]);
    } else {
        crate::data::tmux_set(&["set-option", "-p", "-t", pane, OPTION, &text]);
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
        crate::data::tmux(&at)
    };
    let dir = pane.filter(|p| is_pane(p)).and_then(folder);
    let dir = dir.map_or_else(String::new, |d| d.trim_end_matches('\n').to_string());
    let one = std::slice::from_ref(r);
    // from `/`, render writes the path without its leading slash
    if dir.is_empty() {
        format!("/{}", refs::render(one, Path::new("/")))
    } else {
        refs::render(one, Path::new(&dir))
    }
}

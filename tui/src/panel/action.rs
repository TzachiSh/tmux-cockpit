//! The action, text, cmd and state files the panel hands its wrapper, and the
//! `--resume` state read back.
use super::app::{Action, Resume};
use super::parse::Step;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

const STATE_HEAD: &str = "cockpit-panel-state 1";
/// Far over any real reply: write.txt asks for a handful of steps.
const STATE_MAX: u64 = 64 * 1024;
const STATE_STEPS: usize = 64;

/// Control characters out, `\t` and `\n` kept, `\r\n` folded: a stray ESC or
/// C1 byte pasted into a pane is a keystroke, not text.
pub fn sanitize(s: &str) -> String {
    s.replace("\r\n", "\n")
        .chars()
        .filter(|&c| c == '\t' || c == '\n' || !c.is_control())
        .collect()
}

/// `<path>.text` first, then the action line, so the wrapper never reads a
/// line whose text is not there yet. Both are created fresh, never followed.
/// `<path>.cmd` is the command the pane ran when it was captured, for the
/// wrapper to check it still runs before the paste lands.
pub fn write_action(path: &Path, pane: &str, cmd: &str, a: &Action) -> std::io::Result<()> {
    fresh(Path::new(&format!("{}.text", path.display())), &sanitize(&a.text))?;
    fresh(
        Path::new(&format!("{}.cmd", path.display())),
        &format!("{}\n", sanitize(cmd).replace('\n', " ")),
    )?;
    fresh(path, &format!("{} {pane}\n", a.verb))
}

fn fresh(p: &Path, body: &str) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .mode(0o600)
        .open(p)?;
    f.write_all(body.as_bytes())
}

fn state_esc(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn state_unesc(s: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            _ => return Err("a bad escape".into()),
        }
    }
    Ok(out)
}

/// `<path>.state`, for the wrapper to hand back with `--resume`: written before
/// the action line, created fresh and never followed, like the other two.
pub fn write_state(path: &Path, pane: &str, cmd: &str, r: &Resume) -> std::io::Result<()> {
    let b = state_body(pane, cmd, r).ok_or(std::io::Error::new(std::io::ErrorKind::InvalidInput, "steps too long to keep"))?;
    fresh(Path::new(&format!("{}.state", path.display())), &b)
}

/// The state file's text, or `None` when [`read_state`] would refuse it.
pub fn state_body(pane: &str, cmd: &str, r: &Resume) -> Option<String> {
    if r.items.len() > STATE_STEPS {
        return None;
    }
    let mut b = format!(
        "{STATE_HEAD}\npane {pane}\ncmd {}\nlabel {}\nsel {}\nverb {}\n",
        state_esc(cmd),
        state_esc(&r.label),
        r.sel,
        r.verb
    );
    for (s, d) in r.items.iter().zip(&r.done) {
        let f = [&s.lang, &s.note, &s.cmd].map(|x| state_esc(x));
        b.push_str(&format!("step {}\t{}\t{}\t{}\n", u8::from(*d), f[0], f[1], f[2]));
    }
    (b.len() as u64 <= STATE_MAX).then_some(b)
}

/// The command and the steps a [`write_state`] file holds, refused unless it is
/// a small regular file of this version, for this very pane.
pub fn read_state(path: &Path, pane: &str) -> Result<(String, Resume), String> {
    use std::io::Read;
    let small = |m: std::fs::Metadata| m.is_file() && m.len() <= STATE_MAX;
    // checked before the open, which a FIFO would block, and again on the fd itself
    if !std::fs::symlink_metadata(path).is_ok_and(small) {
        return Err("not a small regular file".into());
    }
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| e.to_string())?;
    if !f.metadata().is_ok_and(small) {
        return Err("not a small regular file".into());
    }
    let mut body = String::new();
    f.take(STATE_MAX).read_to_string(&mut body).map_err(|e| e.to_string())?;
    let Some(body) = body.strip_suffix('\n') else {
        return Err("cut short".into());
    };
    let mut lines = body.split('\n');
    if lines.next() != Some(STATE_HEAD) {
        return Err("not a version 1 state file".into());
    }
    let mut field = |key: &str| -> Result<String, String> {
        let l = lines.next().ok_or(format!("no {key} line"))?;
        let v = l
            .strip_prefix(key)
            .and_then(|v| v.strip_prefix(' '))
            .ok_or(format!("{key} expected"))?;
        state_unesc(v)
    };
    if field("pane")? != pane {
        return Err(format!("it is for another pane, not {pane}"));
    }
    let cmd = field("cmd")?;
    let label = field("label")?;
    let sel: usize = field("sel")?.parse().map_err(|_| "a bad sel".to_string())?;
    let verb = match field("verb")?.as_str() {
        "paste" => "paste",
        "send" => "send",
        _ => return Err("a bad verb".into()),
    };
    let (mut items, mut done) = (vec![], vec![]);
    for l in lines {
        let rest = l.strip_prefix("step ").ok_or("a line that is not a step")?;
        let f: Vec<&str> = rest.split('\t').collect();
        let [d, lang, note, cmd] = f[..] else {
            return Err("a step needs four fields".into());
        };
        done.push(match d {
            "0" => false,
            "1" => true,
            _ => return Err("a bad done mark".into()),
        });
        items.push(Step {
            lang: state_unesc(lang)?,
            note: state_unesc(note)?,
            cmd: state_unesc(cmd)?,
        });
    }
    if items.is_empty() || items.len() > STATE_STEPS || sel >= items.len() {
        return Err("no step to come back to".into());
    }
    Ok((
        cmd,
        Resume {
            label,
            items,
            sel,
            done,
            verb,
        },
    ))
}

//! The system prompts, shipped in the binary. A file of the same name under
//! `$XDG_CONFIG_HOME/tmux-cockpit/prompts/` wins over the shipped text.
use super::envelope::OutType;

pub const WRITE: &str = include_str!("../../prompts/write.txt");
pub const WRITE_EXAMPLES: &str = include_str!("../../prompts/write.examples.txt");
pub const EXPLAIN: &str = include_str!("../../prompts/explain.txt");
pub const FIX: &str = include_str!("../../prompts/fix.txt");

/// The core examples always go; the rest of the 9 come from `when`, then `fill`.
const SENT: usize = 9;
const EXTRA: usize = 2;

/// What the write examples are picked by. `guess` is a local type guess, or
/// `Auto` when there is none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ctx {
    pub out: OutType,
    pub guess: OutType,
    pub window: &'static str,
}

/// `name` is the file stem: write, write.examples, explain or fix. An override
/// that exists but cannot be read falls back to `shipped`, and the note says why.
pub fn load(name: &str, shipped: &str) -> (String, Option<String>) {
    let p = super::xdg("XDG_CONFIG_HOME", ".config").join(format!("tmux-cockpit/prompts/{name}.txt"));
    let (text, note) = match std::fs::read_to_string(&p) {
        Ok(t) => (t, None),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (shipped.to_string(), None),
        Err(e) => {
            let note = format!("cannot read {}: {e}; using the shipped prompt", p.display());
            eprintln!("cockpit-tui: {note}");
            (shipped.to_string(), Some(note))
        }
    };
    (text.trim_end_matches('\n').to_string(), note)
}

/// The write prompt, and a note for each override that could not be read.
pub fn write_noted(ctx: &Ctx) -> (String, Option<String>) {
    let (pool, n1) = load("write.examples", WRITE_EXAMPLES);
    let (template, n2) = load("write", WRITE);
    let notes: Vec<String> = n2.into_iter().chain(n1).collect();
    let note = (!notes.is_empty()).then(|| notes.join("; "));
    (render(&template, Some(&examples(&pool, ctx))), note)
}

pub fn explain_noted() -> (String, Option<String>) {
    let (t, note) = load("explain", EXPLAIN);
    (render(&t, None), note)
}

pub fn fix_noted() -> (String, Option<String>) {
    let (t, note) = load("fix", FIX);
    (render(&t, None), note)
}

pub fn write(ctx: &Ctx) -> String {
    write_noted(ctx).0
}

pub fn explain() -> String {
    explain_noted().0
}

pub fn fix() -> String {
    fix_noted().0
}

pub fn platform() -> String {
    let os = if cfg!(target_os = "macos") { "macOS" } else { "Linux" };
    format!("a {os} terminal (tmux; windows run Claude Code, shells, editors and ssh sessions)")
}

const ABOUT_USER: &str = "The user may type fast, so expect typos and missing words.";

/// Fills `{{platform}}`, `{{about_user}}`, `{{home}}` and `{{examples}}`; twice,
/// because the examples carry `{{home}}`. An unknown name is left as written.
pub fn render(template: &str, examples: Option<&str>) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let platform = platform();
    let mut vals = vec![("platform", platform.as_str()), ("home", &home), ("about_user", ABOUT_USER)];
    vals.extend(examples.map(|e| ("examples", e)));
    fill(&fill(template, &vals), &vals)
}

fn fill(s: &str, vals: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("{{") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        let n = after.bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_').count();
        match vals.iter().find(|(k, _)| *k == &after[..n]) {
            Some((_, v)) if n > 0 && after[n..].starts_with("}}") => {
                out.push_str(v);
                rest = &after[n + 2..];
            }
            _ => {
                out.push_str("{{");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

struct Example<'a> {
    attrs: Vec<(&'a str, &'a str)>,
    body: &'a str,
}

impl Example<'_> {
    fn get(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(a, _)| *a == k).map(|(_, v)| *v)
    }
}

/// `<example k="v" ...>body</example>`; the pool's comment and attributes are
/// never sent, only the bodies.
fn pool(text: &str) -> Vec<Example<'_>> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(i) = text[at..].find("<example").map(|i| at + i) {
        at = i + 8;
        let rest = &text[at..];
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let Some(gt) = rest.find('>') else { break };
        let Some(close) = rest[gt + 1..].find("</example>") else { break };
        out.push(Example {
            attrs: attrs(&rest[..gt]),
            body: &rest[gt + 1..gt + 1 + close],
        });
        at += gt + 1 + close + 10;
    }
    out
}

fn attrs(s: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(eq) = rest.find("=\"") {
        let key_start = rest[..eq]
            .char_indices()
            .rev()
            .take_while(|(_, c)| super::is_word(*c))
            .last()
            .map_or(eq, |(k, _)| k);
        let v = &rest[eq + 2..];
        let Some(q) = v.find('"') else { break };
        if key_start < eq {
            out.push((&rest[key_start..eq], &v[..q]));
        }
        rest = &v[q + 1..];
    }
    out
}

/// The `<examples>` block for the write prompt, in pool order.
pub fn examples(pool_text: &str, ctx: &Ctx) -> String {
    let pool = pool(pool_text);
    let core: Vec<usize> = (0..pool.len())
        .filter(|&i| pool[i].get("core").is_some_and(|v| !v.is_empty()))
        .collect();
    let want: Vec<String> = [
        (ctx.out != OutType::Auto).then(|| format!("out:{}", ctx.out.as_str())),
        Some(format!("guess:{}", ctx.guess.as_str())),
        Some(format!("window:{}", ctx.window)),
    ]
    .into_iter()
    .flatten()
    .collect();
    let mut extra: Vec<usize> = Vec::new();
    for w in &want {
        for (i, e) in pool.iter().enumerate() {
            if !core.contains(&i) && !extra.contains(&i) && e.get("when").unwrap_or("").split_whitespace().any(|x| x == w) {
                extra.push(i);
            }
        }
    }
    extra.truncate(EXTRA);
    let mut fills: Vec<(i64, usize)> = (0..pool.len())
        .filter_map(|i| Some((pool[i].get("fill")?.parse().ok()?, i)))
        .collect();
    fills.sort();
    for (_, i) in fills {
        if core.len() + extra.len() >= SENT {
            break;
        }
        if !core.contains(&i) && !extra.contains(&i) {
            extra.push(i);
        }
    }
    let bodies: Vec<String> = (0..pool.len())
        .filter(|i| core.contains(i) || extra.contains(i))
        .take(SENT)
        .map(|i| format!("<example>{}</example>", pool[i].body))
        .collect();
    format!("<examples>\n{}\n</examples>", bodies.join("\n"))
}

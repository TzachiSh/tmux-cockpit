//! The user prompt sent on stdin: tagged parts joined by blank lines.
use super::action::sanitize;
use super::ask::AskCode;
use super::capture::{Capture, is_claude, session_state};
use super::redact::{added, redact_clean};
use super::request::read_denied;
use crate::data::workdir::Source;
use crate::refs::Ref;
use std::path::Path;

/// What Write is asked to make; `Auto` lets Claude choose and sends no tag.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OutType {
    #[default]
    Auto,
    Prompt,
    Steps,
    Command,
    Text,
    Format,
    Hint,
}

impl OutType {
    pub fn as_str(self) -> &'static str {
        match self {
            OutType::Auto => "auto",
            OutType::Prompt => "prompt",
            OutType::Steps => "steps",
            OutType::Command => "command",
            OutType::Text => "text",
            OutType::Format => "format",
            OutType::Hint => "hint",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Flow {
    Write {
        draft: String,
        out: OutType,
    },
    Fix {
        draft: String,
    },
    Explain,
    /// the viewer's `?` (#365): a question about lines of a file
    Ask(AskCode),
}

impl Flow {
    /// A question and its answers, the thread kept for follow-ups: Explain, and
    /// the viewer's ask. Write and Fix make text to paste instead.
    pub fn is_qa(&self) -> bool {
        matches!(self, Flow::Explain | Flow::Ask(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    pub role: Role,
    pub text: String,
}

/// The tags the envelope wraps text in: every tag [`build_refs`] opens, so
/// [`esc`] escapes a spaced closer of each (`tests/panel_closers.rs`).
pub const SECTIONS: [&str; 15] = [
    "screen",
    "window",
    "repo",
    "refs",
    "ref",
    "request",
    "output_type",
    "text",
    "code",
    "truncated",
    "session",
    "agent",
    "conversation",
    "turn",
    "questions_left",
];

/// User and screen text must not be able to close one of our tags: every `</`
/// is escaped, and so is a closer of a [`SECTIONS`] name with blanks before or
/// after its `/` (`< /screen>`, #671). A spaced closer of any other name stays,
/// as does a `<` and `/` in ordinary text: escaping every `< /` would change
/// screen text that closes nothing of ours. The escape is one more `\` before
/// the `/`, so a closer the text had already escaped (`<\/div>`) gets two and
/// [`unesc`] gives back what the text had (#681).
pub fn esc(s: &str) -> String {
    shift_closers(s, &SECTIONS, |c| c.backslashes + usize::from(c.escaped()))
}

/// [`esc`] undone, for text the model copied from the envelope: one `\` fewer
/// before the slash of a closer [`esc`] escapes. No other `\` changes.
pub fn unesc(s: &str) -> String {
    shift_closers(s, &SECTIONS, |c| c.backslashes - usize::from(c.escaped() && c.backslashes > 0))
}

/// A closing tag's slash: after a `<`, blanks and `\`s, before blanks and a name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Closer {
    /// blanks between the `<` and the `\`s or the `/`
    pub spaced: bool,
    /// the name is one of the names [`shift_closers`] was given, in any case
    pub ours: bool,
    /// the `\`s just before the `/`
    pub backslashes: usize,
}

impl Closer {
    /// The ones [`esc`] escapes: every `</`, and a spaced closer of a section.
    fn escaped(self) -> bool {
        self.ours || !self.spaced
    }
}

/// `s` with the `\`s before each closer's slash made as many as `to` says. A
/// name going on with a letter, digit, `_`, `-` or `.` is another tag. [`esc`],
/// [`unesc`] and the eval judge's prompt share it, each with its own names.
pub fn shift_closers(s: &str, names: &[&str], to: impl Fn(Closer) -> usize) -> String {
    let name_char = |c: &u8| c.is_ascii_alphanumeric() || b"_-.".contains(c);
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('<') {
        out.push_str(&rest[..=at]);
        rest = &rest[at + 1..];
        let slash = rest.trim_start();
        let backslashes = slash.bytes().take_while(|&b| b == b'\\').count();
        let Some(after) = slash[backslashes..].strip_prefix('/') else {
            continue;
        };
        let name = after.trim_start().as_bytes();
        let ours = names.iter().any(|t| {
            let t = t.as_bytes();
            let at_end = !name.get(t.len()).is_some_and(name_char);
            name.len() >= t.len() && name[..t.len()].eq_ignore_ascii_case(t) && at_end
        });
        let spaced = slash.len() < rest.len();
        out.push_str(&rest[..rest.len() - slash.len()]);
        out.push_str(&"\\".repeat(to(Closer { spaced, ours, backslashes })));
        out.push('/');
        rest = after;
    }
    out + rest
}

/// The draft is not redacted: Fix types its answer back, so a blanked draft
/// would come back blanked. Turns are, since an answer may carry a secret.
pub fn build(flow: &Flow, cap: &Capture, turns: &[Turn], questions_left: usize) -> String {
    build_counted(flow, cap, turns, questions_left).0
}

/// [`build`], and how many secrets the screen and its window, the repo block
/// and the turns had hidden.
pub fn build_counted(flow: &Flow, cap: &Capture, turns: &[Turn], questions_left: usize) -> (String, usize) {
    let e = build_parts(flow, cap, turns, questions_left);
    let n = e.hidden();
    (e.text, n)
}

/// The envelope and its hidden secrets, counted by where they were.
#[derive(Clone, Debug, PartialEq)]
pub struct Envelope {
    pub text: String,
    pub screen_hidden: usize,
    pub repo_hidden: usize,
    pub turns_hidden: usize,
}

impl Envelope {
    /// The secrets hidden, wherever they were.
    pub fn hidden(&self) -> usize {
        self.screen_hidden + self.repo_hidden + self.turns_hidden
    }
}

/// [`build_counted`] with the count split. Only a Write waits for the repo block.
pub fn build_parts(flow: &Flow, cap: &Capture, turns: &[Turn], questions_left: usize) -> Envelope {
    build_refs(flow, cap, turns, questions_left, &[])
}

/// One basket ref as a Write request names it (#365 child 5): `text` in the
/// form the pane's agent reads ([`crate::basket::ref_line`]), whether it names
/// a whole file, and whether it is outside the folder an exploring write may read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefItem {
    pub text: String,
    /// `text` is a Claude Code `@` mention; else a bare path (#590)
    pub mention: bool,
    pub whole: bool,
    pub outside: bool,
    /// inside the root, but a file the read-only profile denies (a secret)
    pub denied: bool,
}

/// `refs` as [`RefItem`]s, from `base` (the pane's folder). `root` is the
/// folder an exploring write may read, `None` for a write that reads nothing.
/// A ref's path is canonical (`Ref::new`), so a symlink was followed when it
/// was added: one inside the root that leads out is outside, as the read would
/// be refused, and one outside that leads in is inside. The root is compared
/// canonical too, so a pane reached through a link is the folder it names.
/// A ref inside the root that the read-only profile denies by rule (a `.env`,
/// a key under `~/.ssh`: `request::read_denied`) is noted as not read too.
pub fn ref_items(refs: &[Ref], base: &Path, claude: bool, root: Option<&Path>) -> Vec<RefItem> {
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let root = root.map(canon);
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty()).map(|h| canon(Path::new(&h)));
    let inside = |r: &Ref| root.as_ref().is_some_and(|root| r.path().starts_with(root));
    refs.iter()
        .map(|r| {
            let text = crate::basket::ref_line(r, base, claude);
            RefItem {
                // ref_line falls back to the bare form for a path a mention cannot carry
                mention: claude && text != crate::refs::render(std::slice::from_ref(r), base),
                text,
                whole: r.lines().is_none(),
                outside: root.is_some() && !inside(r),
                denied: inside(r) && read_denied(r.path(), home.as_deref()),
            }
        })
        .collect()
}

/// The `<refs>` block: one `<ref>` a line, its form (`mention` or `bare`, #590:
/// the model copies it rather than turning a bare ref into a mention), its note
/// saying a whole file or one outside the folder the write may read. Cleaned as a send is
/// (`action::sanitize`), and escaped, so a name cannot close the tag.
fn refs_part(refs: &[RefItem]) -> String {
    let lines: Vec<String> = refs
        .iter()
        .map(|r| {
            let notes: Vec<&str> = [
                (r.whole, "whole file"),
                (r.outside, "outside the repo, not read"),
                (r.denied, "a secret the write may not read, not read"),
            ]
            .into_iter()
            .filter_map(|(on, n)| on.then_some(n))
            .collect();
            let note = if notes.is_empty() {
                String::new()
            } else {
                format!(" note=\"{}\"", notes.join("; "))
            };
            let form = if r.mention { "mention" } else { "bare" };
            format!("<ref form=\"{form}\"{note}>{}</ref>", esc(&sanitize(&r.text)))
        })
        .collect();
    format!("<refs>\n{}\n</refs>", lines.join("\n"))
}

/// The screen and the window's name as every `<screen>` sends them, redacted,
/// and how many secrets the two hid. The panel's Home line counts with this
/// call too (#964), so it cannot count the screen another way than it is sent.
pub fn redact_screen(cap: &Capture) -> (String, String, usize) {
    let (screen, n) = redact_clean(&cap.screen);
    let (name, m) = redact_clean(&cap.name);
    (screen, name, n + m)
}

/// The work folder as the `<window>` line names it after the cwd (#1353, amendment 13 on
/// #1342), and the secrets it hid: the folder and its source when it is not the cwd. On an
/// agent's window (a claude name, or the board's facts) whose source is a guess
/// (active-in, pane-cwd), the folder is marked not confirmed, at the cwd too. Else
/// nothing, as for a plain shell, whose cwd is its folder. It runs no process.
fn window_work(cap: &Capture) -> (String, usize) {
    let w = &cap.work;
    if w.dir.is_empty() {
        return (String::new(), 0);
    }
    let agent = cap.agent.is_some() || is_claude(&cap.name, &cap.cmd);
    let guess = agent && matches!(w.source, Source::ActiveIn | Source::PaneCwd);
    let src = w.source.as_str();
    if w.dir != cap.path {
        let (dir, n) = redact_clean(&w.dir);
        let mark = if guess { ", not confirmed" } else { "" };
        return (format!(" · work folder {} ({src}{mark})", esc(&dir)), n);
    }
    if guess {
        (format!(" · work folder not confirmed ({src})"), 0)
    } else {
        (String::new(), 0)
    }
}

/// How many characters of `doing` and of `asked` Explain's `<agent>` part keeps
/// (amendment 13 on #1342).
const AGENT_TEXT_MAX: usize = 300;

/// Explain's `<agent>` part (#1353): the board's state and model as attributes, then its
/// doing, asked and the work folder with its source, a line each when known. The text and
/// the model are the agent's, so they are redacted, cleaned and escaped as the screen is,
/// and the secrets they hid come back with it. `doing` and `asked` keep their first
/// [`AGENT_TEXT_MAX`] characters, cut after the redaction so a cut never splits a secret
/// out of its match. `None` without the board's facts.
fn agent_part(cap: &Capture) -> Option<(String, usize)> {
    let a = cap.agent.as_ref()?;
    let state = if a.state.is_empty() { "unknown" } else { &a.state };
    let attr = |s: &str| esc(s).replace('"', "&quot;");
    let mut head = format!("<agent state=\"{}\"", attr(&sanitize(state)));
    let (model, mut hidden) = redact_clean(&a.model);
    if !model.is_empty() {
        head.push_str(&format!(" model=\"{}\"", attr(&model)));
    }
    let mut body = String::new();
    let mut line = |label: &str, text: &str, cut: usize, tail: &str| {
        if !text.is_empty() {
            let (t, n) = redact_clean(text);
            hidden += n;
            let t: String = t.chars().take(cut).collect();
            body.push_str(&format!("{label}: {}{tail}\n", esc(&t)));
        }
    };
    line("doing", &a.doing, AGENT_TEXT_MAX, "");
    line("asked", &a.asked, AGENT_TEXT_MAX, "");
    let src = format!(" ({})", cap.work.source.as_str());
    line("work folder", &cap.work.dir, usize::MAX, &src);
    Some((format!("{head}>\n{body}</agent>"), hidden))
}

/// [`build_parts`], and for a Write with the basket's refs (#365 child 5) a
/// `<refs>` block before the request. No other flow carries it.
///
/// Every section of outside text is cleaned as a send is ([`sanitize`], #936)
/// and escaped last. A redacted section is redacted both before and after the
/// cleaning ([`redact_clean`]): a direction control can split a key or be the
/// boundary before one.
///
/// Redacted: the screen, the window's name and the pane's path (#937), the
/// work folder and Explain's `<agent>` text (#1353, counted in `screen_hidden`), the
/// repo block, an Ask's code and the turns. The window's command is a
/// program's name (`#{pane_current_command}`, never the start command), so it
/// is only cleaned. A ref and an Ask's path are files the user chose, and are
/// only cleaned too. The name and the path count in `screen_hidden`: they
/// label the screen. The name counts once though a write writes it twice. Only
/// a flow that sends the screen counts it: an Ask sends neither it nor the
/// window (#965), and counts its code in `repo_hidden`.
pub fn build_refs(flow: &Flow, cap: &Capture, turns: &[Turn], questions_left: usize, refs: &[RefItem]) -> Envelope {
    let (screen, name, on_screen) = redact_screen(cap);
    let (mut screen_hidden, mut repo_hidden, mut turns_hidden) = (0, 0, 0);
    let screen = esc(&screen);
    let (name, cmd) = (esc(&name), esc(&sanitize(&cap.cmd)));
    // A `"` in a window name would end the attribute early.
    let window = format!("{name} · {cmd}").replace('"', "&quot;");
    let mut parts = Vec::new();
    match flow {
        Flow::Write { draft, out } => {
            let shown = if screen.is_empty() { "(empty)" } else { &screen };
            parts.push(format!("<screen window=\"{window}\">\n{shown}\n</screen>"));
            let (path, path_hidden) = redact_clean(&cap.path);
            let (work, work_hidden) = window_work(cap);
            screen_hidden = on_screen + path_hidden + work_hidden;
            parts.push(format!("<window>{name} · {cmd} · {}{work}</window>", esc(&path)));
            let repo = cap.repo.get();
            if !repo.is_empty() {
                // Its commit messages and file names carry secrets as a screen
                // does. The window's name and path are redacted above; a ref
                // and an Ask's path are not (the list before this function).
                let (repo, n) = redact_clean(repo);
                repo_hidden = n;
                parts.push(format!("<repo>\n{}\n</repo>", esc(&repo)));
            }
            if !refs.is_empty() {
                parts.push(refs_part(refs));
            }
            parts.push(format!("<request>{}</request>", esc(&sanitize(draft))));
            if *out != OutType::Auto {
                parts.push(format!("<output_type>{}</output_type>", out.as_str()));
            }
        }
        Flow::Fix { draft } => {
            screen_hidden = on_screen;
            parts.push(format!("<screen window=\"{window}\">\n{screen}\n</screen>"));
            parts.push(format!("<text>{}</text>", esc(&sanitize(draft))));
        }
        // the code, never the screen: the question is about the file (#365)
        Flow::Ask(a) => {
            // ask::load cleaned and redacted each line before it cut any; a
            // state file's code is done here too, counting only what is new
            let (code, _) = redact_clean(&a.code);
            repo_hidden = a.hidden + added(&a.code, &code);
            let lines = format!("{}-{}", a.asked.0, a.asked.1);
            // ask::load refused a name with a control character; a `"` would end the attribute
            let path = a.path.replace('"', "&quot;");
            let code = esc(&code);
            let path = esc(&path);
            parts.push(format!("<code path=\"{path}\" lines=\"{lines}\">\n{code}\n</code>"));
            let (s0, s1) = a.shown;
            let tail = "Read the file for the rest</truncated>";
            if a.shown != a.asked {
                let short = if a.cut { format!(", line {s1} cut short") } else { String::new() };
                let shown = format!("lines {s0}-{s1} of {lines} are shown{short}");
                parts.push(format!("<truncated>{shown}; {tail}"));
            } else if a.cut {
                parts.push(format!("<truncated>line {s1} is cut short; {tail}"));
            }
        }
        Flow::Explain => {
            screen_hidden = on_screen;
            parts.push(format!("<screen window=\"{window}\">\n{screen}\n</screen>"));
            if let Some(st) = session_state(&cap.screen, &cap.name, &cap.cmd) {
                parts.push(format!(
                    "<session app=\"claude-code\" state=\"{}\">The window runs Claude Code; it {}.</session>",
                    st.as_str(),
                    st.text()
                ));
            }
            if let Some((part, n)) = agent_part(cap) {
                screen_hidden += n;
                parts.push(part);
            }
        }
    }
    if !turns.is_empty() {
        let lines: Vec<String> = turns
            .iter()
            .map(|t| {
                let role = if t.role == Role::User { "user" } else { "assistant" };
                let (text, n) = redact_clean(&t.text);
                turns_hidden += n;
                format!("<turn role=\"{role}\">{}</turn>", esc(&text))
            })
            .collect();
        parts.push(format!("<conversation>\n{}\n</conversation>", lines.join("\n")));
    }
    if !matches!(flow, Flow::Fix { .. }) {
        parts.push(format!("<questions_left>{questions_left}</questions_left>"));
    }
    Envelope {
        text: parts.join("\n\n"),
        screen_hidden,
        repo_hidden,
        turns_hidden,
    }
}

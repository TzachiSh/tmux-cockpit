//! The user prompt sent on stdin: tagged parts joined by blank lines.
use super::ask::AskCode;
use super::capture::{Capture, session_state};
use super::redact::redact;

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

/// User and screen text must not be able to close one of our tags.
pub fn esc(s: &str) -> String {
    s.replace("</", "<\\/")
}

pub fn unesc(s: &str) -> String {
    s.replace("<\\/", "</")
}

/// The draft is not redacted: Fix types its answer back, so a blanked draft
/// would come back blanked. Turns are, since an answer may carry a secret.
pub fn build(flow: &Flow, cap: &Capture, turns: &[Turn], questions_left: usize) -> String {
    build_counted(flow, cap, turns, questions_left).0
}

/// [`build`], and how many secrets the screen, the repo block and the turns had hidden.
pub fn build_counted(flow: &Flow, cap: &Capture, turns: &[Turn], questions_left: usize) -> (String, usize) {
    let e = build_parts(flow, cap, turns, questions_left);
    (e.text, e.screen_hidden + e.repo_hidden + e.turns_hidden)
}

/// The envelope and its hidden secrets, counted by where they were.
#[derive(Clone, Debug, PartialEq)]
pub struct Envelope {
    pub text: String,
    pub screen_hidden: usize,
    pub repo_hidden: usize,
    pub turns_hidden: usize,
}

/// [`build_counted`] with the count split. Only a Write waits for the repo block.
pub fn build_parts(flow: &Flow, cap: &Capture, turns: &[Turn], questions_left: usize) -> Envelope {
    let (screen, screen_hidden) = redact(&cap.screen);
    let (mut repo_hidden, mut turns_hidden) = (0, 0);
    let screen = esc(&screen);
    let (name, cmd) = (esc(&cap.name), esc(&cap.cmd));
    // A `"` in a window name would end the attribute early.
    let window = format!("{name} · {cmd}").replace('"', "&quot;");
    let mut parts = Vec::new();
    match flow {
        Flow::Write { draft, out } => {
            let shown = if screen.is_empty() { "(empty)" } else { &screen };
            parts.push(format!("<screen window=\"{window}\">\n{shown}\n</screen>"));
            parts.push(format!("<window>{name} · {cmd} · {}</window>", esc(&cap.path)));
            let repo = cap.repo.get();
            if !repo.is_empty() {
                // Commit messages and paths carry secrets as a screen does.
                let (repo, n) = redact(repo);
                repo_hidden = n;
                parts.push(format!("<repo>\n{}\n</repo>", esc(&repo)));
            }
            parts.push(format!("<request>{}</request>", esc(draft)));
            if *out != OutType::Auto {
                parts.push(format!("<output_type>{}</output_type>", out.as_str()));
            }
        }
        Flow::Fix { draft } => {
            parts.push(format!("<screen window=\"{window}\">\n{screen}\n</screen>"));
            parts.push(format!("<text>{}</text>", esc(draft)));
        }
        // the code, never the screen: the question is about the file (#365)
        Flow::Ask(a) => {
            // ask::load redacted each line before it cut any
            repo_hidden = a.hidden;
            let lines = format!("{}-{}", a.asked.0, a.asked.1);
            // ask::load refused a name with a control character; a `"` would end the attribute
            let path = a.path.replace('"', "&quot;");
            let code = esc(&a.code);
            parts.push(format!("<code path=\"{}\" lines=\"{lines}\">\n{code}\n</code>", esc(&path)));
            let (s0, s1) = a.shown;
            let tail = "Read the file for the rest</truncated>";
            if a.shown != a.asked {
                let short = if a.cut { format!(", line {s1} cut short") } else { String::new() };
                parts.push(format!("<truncated>lines {s0}-{s1} of {lines} are shown{short}; {tail}"));
            } else if a.cut {
                parts.push(format!("<truncated>line {s1} is cut short; {tail}"));
            }
        }
        Flow::Explain => {
            parts.push(format!("<screen window=\"{window}\">\n{screen}\n</screen>"));
            if let Some(st) = session_state(&cap.screen, &cap.name, &cap.cmd) {
                parts.push(format!(
                    "<session app=\"claude-code\" state=\"{}\">The window runs Claude Code; it {}.</session>",
                    st.as_str(),
                    st.text()
                ));
            }
        }
    }
    if !turns.is_empty() {
        let lines: Vec<String> = turns
            .iter()
            .map(|t| {
                let role = if t.role == Role::User { "user" } else { "assistant" };
                let (text, n) = redact(&t.text);
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

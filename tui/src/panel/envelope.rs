//! The user prompt sent on stdin: tagged parts joined by blank lines.
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
    Write { draft: String, out: OutType },
    Fix { draft: String },
    Explain,
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
    let screen = esc(&redact(&cap.screen).0);
    let (name, cmd) = (esc(&cap.name), esc(&cap.cmd));
    // A `"` in a window name would end the attribute early.
    let window = format!("{name} · {cmd}").replace('"', "&quot;");
    let mut parts = Vec::new();
    match flow {
        Flow::Write { draft, out } => {
            let shown = if screen.is_empty() { "(empty)" } else { &screen };
            parts.push(format!("<screen window=\"{window}\">\n{shown}\n</screen>"));
            parts.push(format!("<window>{name} · {cmd} · {}</window>", esc(&cap.path)));
            parts.push(format!("<request>{}</request>", esc(draft)));
            if *out != OutType::Auto {
                parts.push(format!("<output_type>{}</output_type>", out.as_str()));
            }
        }
        Flow::Fix { draft } => {
            parts.push(format!("<screen window=\"{window}\">\n{screen}\n</screen>"));
            parts.push(format!("<text>{}</text>", esc(draft)));
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
                format!("<turn role=\"{role}\">{}</turn>", esc(&redact(&t.text).0))
            })
            .collect();
        parts.push(format!("<conversation>\n{}\n</conversation>", lines.join("\n")));
    }
    if !matches!(flow, Flow::Fix { .. }) {
        parts.push(format!("<questions_left>{questions_left}</questions_left>"));
    }
    parts.join("\n\n")
}

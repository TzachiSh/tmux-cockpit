//! Which model answers: a small rule table, the fast model unless a mistake
//! costs more or the request is long.
use super::envelope::{Flow, OutType};
use super::{boundary_at, boundary_before, ci};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub model: String,
    pub why: String,
    /// `(model, why)` when the rules want a stronger model than the one chosen.
    pub suggest: Option<(String, String)>,
}

/// `default` is the account default, counted as the strongest.
fn strength(m: &str) -> Option<u8> {
    match m {
        "haiku" => Some(1),
        "sonnet" => Some(2),
        "opus" | "default" => Some(3),
        _ => None,
    }
}

/// `--model` for `claude -p`; `default` or nothing means no flag.
pub fn model_argv(model: &str) -> Vec<String> {
    if model.is_empty() || model == "default" {
        vec![]
    } else {
        vec!["--model".into(), model.into()]
    }
}

/// `choice` is the user's pick (`auto` for none). `COCKPIT_PANEL_MODEL` beats both.
pub fn route(flow: &Flow, choice: &str) -> Route {
    let (rm, rwhy) = rule(flow);
    if let Ok(m) = std::env::var("COCKPIT_PANEL_MODEL")
        && !m.is_empty()
    {
        return Route {
            model: m,
            why: "COCKPIT_PANEL_MODEL is set".into(),
            suggest: None,
        };
    }
    if choice.is_empty() || choice == "auto" {
        return Route {
            model: rm.into(),
            why: rwhy.into(),
            suggest: None,
        };
    }
    let stronger = strength(rm).unwrap_or(0) > strength(choice).unwrap_or(3);
    Route {
        model: choice.into(),
        why: "chosen on this screen".into(),
        suggest: stronger.then(|| (rm.into(), rwhy.into())),
    }
}

fn rule(flow: &Flow) -> (&'static str, &'static str) {
    match flow {
        Flow::Write { out: OutType::Steps, .. } => ("sonnet", "several commands you will run - needs precision"),
        Flow::Write { draft, .. } if draft.chars().count() > 400 || request_steps(draft) >= 3 => ("sonnet", "long, multi-step request"),
        Flow::Write { .. } => ("haiku", "short request - the fast model is enough"),
        Flow::Fix { .. } => ("haiku", "quick spelling fix"),
        Flow::Explain => ("haiku", "quick screen explanation"),
    }
}

const WORDS: [&str; 5] = ["then", "after that", "and also", "also", "afterwards"];
const HEBREW: [&str; 3] = ["ואז", "אחרי זה", "וגם"];

/// One more than the step markers in the draft: "then", "also", a `;`, a list item.
pub fn request_steps(draft: &str) -> usize {
    let mut n = 0;
    let mut i = 0;
    while let Some(c) = draft[i..].chars().next() {
        match step_at(draft, i) {
            Some(end) => {
                n += 1;
                i = end;
            }
            None => i += c.len_utf8(),
        }
    }
    n + 1
}

fn step_at(s: &str, i: usize) -> Option<usize> {
    let rest = &s[i..];
    let word = WORDS
        .iter()
        .find_map(|w| ci(s, i, w).filter(|&end| boundary_before(s, i) && boundary_at(s, end)));
    if word.is_some() {
        return word;
    }
    if let Some(r) = rest.strip_prefix('\n') {
        // `\n\s*(\d+[.)]|[-*•])\s`: a numbered or bulleted line.
        let t = r.trim_start();
        let d = t.bytes().take_while(u8::is_ascii_digit).count();
        let after = if d > 0 {
            t[d..].strip_prefix(['.', ')'])
        } else {
            t.strip_prefix(['-', '*', '•'])
        };
        if let Some(a) = after
            && let Some(ws) = a.chars().next().filter(|c| c.is_whitespace())
        {
            return Some(s.len() - a.len() + ws.len_utf8());
        }
    }
    if rest.starts_with(';') {
        return Some(i + 1);
    }
    HEBREW.iter().find(|h| rest.starts_with(*h)).map(|h| i + h.len())
}

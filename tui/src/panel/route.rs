//! Which model answers: a small rule table. Write takes sonnet: on the #410
//! eval it asked when due 33/33 to haiku's 30/33 at about the same latency (#414).
use super::envelope::{Flow, OutType};

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
        Flow::Write { .. } => ("sonnet", "rough words into a prompt - sonnet asks and steers better"),
        Flow::Fix { .. } => ("haiku", "quick spelling fix"),
        Flow::Explain => ("haiku", "quick screen explanation"),
    }
}

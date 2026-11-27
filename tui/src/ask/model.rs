//! The open ask's input as data (#1504): every question, header, option label,
//! description, preview and multiSelect. The text stays as the agent wrote it:
//! each view cleans what it draws.
use serde_json::Value;

/// One AskUserQuestion call: its `tool_use` id and its questions, in order.
#[derive(Debug, PartialEq)]
pub struct Ask {
    pub use_id: String,
    pub questions: Vec<Question>,
}

/// One question: its text, its chip header, whether more than one option may be
/// picked, and its options in order.
#[derive(Debug, PartialEq)]
pub struct Question {
    pub text: String,
    pub header: String,
    pub multi: bool,
    pub options: Vec<Choice>,
}

/// One option: its label, its description and the preview shown while it has focus.
#[derive(Debug, PartialEq)]
pub struct Choice {
    pub label: String,
    pub description: String,
    pub preview: Option<String>,
}

/// The ask `use_id`, read from its `input`: `None` on any missing or mistyped field,
/// never a panic. Only `preview` may be absent; counts are not checked.
pub fn parse(use_id: &str, input: &Value) -> Option<Ask> {
    let questions = input["questions"].as_array()?.iter().map(read_question).collect::<Option<_>>()?;
    Some(Ask {
        use_id: use_id.to_string(),
        questions,
    })
}

fn read_question(q: &Value) -> Option<Question> {
    Some(Question {
        text: q["question"].as_str()?.to_string(),
        header: q["header"].as_str()?.to_string(),
        multi: q["multiSelect"].as_bool()?,
        options: q["options"].as_array()?.iter().map(read_choice).collect::<Option<_>>()?,
    })
}

fn read_choice(o: &Value) -> Option<Choice> {
    let preview = match o.get("preview") {
        Some(p) => Some(p.as_str()?.to_string()),
        None => None,
    };
    Some(Choice {
        label: o["label"].as_str()?.to_string(),
        description: o["description"].as_str()?.to_string(),
        preview,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// #1504: three questions of four options each come back field for field, the
    /// second one multiSelect and every odd option with a preview.
    #[test]
    fn three_questions_of_four_options_round_trip() {
        let want = Ask {
            use_id: "toolu_01abc".into(),
            questions: (0..3)
                .map(|q| Question {
                    text: format!("Question {q}?"),
                    header: format!("H{q}"),
                    multi: q == 1,
                    options: (0..4)
                        .map(|k| Choice {
                            label: format!("q{q}o{k}"),
                            description: format!("what q{q}o{k} does"),
                            preview: (k % 2 == 1).then(|| format!("preview of q{q}o{k}")),
                        })
                        .collect(),
                })
                .collect(),
        };
        let option = |c: &Choice| {
            let mut o = json!({"label": c.label, "description": c.description});
            if let Some(p) = &c.preview {
                o["preview"] = json!(p);
            }
            o
        };
        let questions: Vec<Value> = want
            .questions
            .iter()
            .map(|q| {
                let options: Vec<Value> = q.options.iter().map(option).collect();
                json!({"question": q.text, "header": q.header, "multiSelect": q.multi, "options": options})
            })
            .collect();
        let input = json!({"questions": questions, "metadata": {"source": "test"}});
        assert_eq!(parse("toolu_01abc", &input), Some(want));
    }

    /// #1504: each field missing, then each of the wrong type, is no ask; so is input
    /// of the wrong shape. A missing preview is the one field that may be absent.
    #[test]
    fn a_missing_or_mistyped_field_gives_no_ask() {
        let opt = json!({"label": "Yes", "description": "go on", "preview": "p"});
        let q = json!({"question": "Merge?", "header": "Merge", "multiSelect": false, "options": [opt, opt]});
        let input = |q: Value| json!({"questions": [q]});
        assert!(parse("u", &input(q.clone())).is_some(), "the whole shape parses");
        let mut rows = vec![
            ("not an object".to_string(), json!("Merge?")),
            ("no questions".into(), json!({})),
            ("questions not a list".into(), json!({"questions": q})),
            ("a question not an object".into(), json!({"questions": ["Merge?"]})),
        ];
        for k in ["question", "header", "multiSelect", "options"] {
            let (mut gone, mut bad) = (q.clone(), q.clone());
            gone.as_object_mut().unwrap().remove(k);
            bad[k] = json!(7);
            rows.push((format!("no {k}"), input(gone)));
            rows.push((format!("{k} a number"), input(bad)));
        }
        for k in ["label", "description", "preview"] {
            let (mut gone, mut bad) = (q.clone(), q.clone());
            gone["options"][1].as_object_mut().unwrap().remove(k);
            bad["options"][1][k] = json!(7);
            if k == "preview" {
                assert!(parse("u", &input(gone)).is_some(), "an option with no preview");
            } else {
                rows.push((format!("an option with no {k}"), input(gone)));
            }
            rows.push((format!("an option's {k} a number"), input(bad)));
        }
        let mut words = q;
        words["options"] = json!(["Yes", "No"]);
        rows.push(("options that are strings".into(), input(words)));
        for (what, v) in rows {
            assert_eq!(parse("u", &v), None, "{what}");
        }
    }
}

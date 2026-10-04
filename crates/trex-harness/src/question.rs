use std::collections::HashSet;

use anyhow::{Context, bail};
use async_openai::types::responses::{FunctionTool, InputItem};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::agent::output_item;

pub const ASK_USER: &str = "ask_user";
const MAX_QUESTIONS: usize = 4;
const MAX_OPTIONS: usize = 6;

#[derive(Deserialize, Serialize)]
pub struct Question {
    pub question: String,
    pub options: Vec<QuestionOption>,
    pub multi_select: bool,
}

#[derive(Deserialize, Serialize)]
pub struct QuestionOption {
    pub label: String,
    pub description: Option<String>,
}

// a user can pick options, type their own text, or both
pub struct Answer {
    pub selected: Vec<String>,
    pub text: Option<String>,
}

#[derive(Deserialize)]
struct Args {
    questions: Vec<Question>,
}

pub fn definition() -> FunctionTool {
    FunctionTool {
        name: ASK_USER.into(),
        description: Some(format!(
            "Ask the user up to {MAX_QUESTIONS} questions when you need a decision, a preference, or missing information \
             you cannot find yourself. Give options when there are clear choices; the user can always type their own answer. \
             Your turn ends until they reply, so batch everything you need into one call and do not ask what you can find out."
        )),
        parameters: Some(json!({
            "type": "object",
            "properties": {
                "questions": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "question": {"type": "string", "description": "The question, ending with a question mark."},
                            "options": {
                                "type": "array",
                                "description": format!("Choices to offer, 2 to {MAX_OPTIONS}; empty for a free-text question."),
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "label": {"type": "string", "description": "Short choice text."},
                                        "description": {"type": ["string", "null"], "description": "What choosing this means."}
                                    },
                                    "required": ["label", "description"],
                                    "additionalProperties": false
                                }
                            },
                            "multi_select": {"type": "boolean", "description": "Allow choosing more than one option."}
                        },
                        "required": ["question", "options", "multi_select"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["questions"],
            "additionalProperties": false,
        })),
        strict: Some(true),
        ..Default::default()
    }
}

// the schema cannot express every rule, so violations go back to the model as a tool error
pub fn parse(arguments: &str) -> anyhow::Result<Vec<Question>> {
    let args: Args = serde_json::from_str(arguments).context("ask_user arguments are not valid")?;
    if args.questions.is_empty() || args.questions.len() > MAX_QUESTIONS {
        bail!("ask between 1 and {MAX_QUESTIONS} questions");
    }
    for question in &args.questions {
        if question.question.trim().is_empty() {
            bail!("questions must not be empty");
        }
        let count = question.options.len();
        if count == 1 || count > MAX_OPTIONS {
            bail!(
                "{:?} needs 2 to {MAX_OPTIONS} options, or none for free text",
                question.question
            );
        }
        let mut labels = HashSet::new();
        for option in &question.options {
            if option.label.trim().is_empty() || !labels.insert(option.label.trim()) {
                bail!(
                    "options for {:?} need unique, non-empty labels",
                    question.question
                );
            }
        }
    }
    Ok(args.questions)
}

pub fn answer_item(call_id: &str, questions: &[Question], answers: &[Answer]) -> InputItem {
    output_item(call_id.to_owned(), format_answers(questions, answers))
}

fn format_answers(questions: &[Question], answers: &[Answer]) -> String {
    questions
        .iter()
        .enumerate()
        .map(|(index, question)| {
            let answer = answers.get(index);
            let mut parts: Vec<String> = answer.map(|a| a.selected.clone()).unwrap_or_default();
            if let Some(text) = answer.and_then(|a| a.text.as_deref()).map(str::trim)
                && !text.is_empty()
            {
                parts.push(text.to_owned());
            }
            let response = if parts.is_empty() {
                "(no answer)".to_owned()
            } else {
                parts.join(", ")
            };
            format!("Q: {}\nA: {response}", question.question)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn question(options: &[&str]) -> String {
        let options: Vec<_> = options
            .iter()
            .map(|label| json!({"label": label, "description": null}))
            .collect();
        json!({"questions": [{"question": "Which?", "options": options, "multi_select": false}]})
            .to_string()
    }

    #[test]
    fn validates_questions() {
        assert!(parse(&question(&["a", "b"])).is_ok());
        assert!(
            parse(&question(&[])).is_ok(),
            "free text questions have no options"
        );
        assert!(parse(&question(&["only"])).is_err());
        assert!(parse(&question(&["same", "same"])).is_err());
        assert!(parse(&question(&["a", "b", "c", "d", "e", "f", "g"])).is_err());
        assert!(parse(r#"{"questions": []}"#).is_err());
        assert!(parse("not json").is_err());
    }

    #[test]
    fn formats_answers_for_the_model() {
        let questions = parse(
            &json!({"questions": [
                {"question": "Language?", "options": [{"label": "Rust", "description": null}, {"label": "Go", "description": null}], "multi_select": true},
                {"question": "Name?", "options": [], "multi_select": false},
                {"question": "Deadline?", "options": [], "multi_select": false}
            ]})
            .to_string(),
        )
        .unwrap();
        let answers = [
            Answer {
                selected: vec!["Rust".into(), "Go".into()],
                text: Some(" also zig ".into()),
            },
            Answer {
                selected: vec![],
                text: Some("trex".into()),
            },
        ];
        assert_eq!(
            format_answers(&questions, &answers),
            "Q: Language?\nA: Rust, Go, also zig\n\nQ: Name?\nA: trex\n\nQ: Deadline?\nA: (no answer)"
        );
    }
}

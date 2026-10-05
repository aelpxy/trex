use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use trex_harness::{
    history,
    question::{self, Answer},
};
use trex_store::sessions::SessionStatus;
use utoipa::ToSchema;

use super::{
    find_session,
    messages::{Run, run_object},
};
use crate::api::{
    AppState,
    auth::Auth,
    error::{ApiError, ErrorResponse},
};
use crate::runs;

#[derive(Deserialize, ToSchema)]
pub struct CreateAnswers {
    /// One answer per pending question, in order.
    answers: Vec<AnswerInput>,
}

#[derive(Deserialize, ToSchema)]
pub struct AnswerInput {
    /// Labels of the chosen options.
    #[serde(default)]
    selected: Vec<String>,
    /// Free text, alongside or instead of options.
    text: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct PendingQuestion {
    call_id: String,
    pub(super) questions: Vec<question::Question>,
}

#[derive(Serialize, ToSchema)]
pub struct Question {
    question: String,
    /// Empty for a free-text question.
    options: Vec<QuestionOption>,
    multi_select: bool,
}

#[derive(Serialize, ToSchema)]
pub struct QuestionOption {
    label: String,
    description: Option<String>,
}

/// Answer questions
///
/// Answers the session's `pending_questions` and resumes the agent.
#[utoipa::path(
    post,
    operation_id = "create_answers",
    path = "/sessions/{id}/answers",
    tag = "sessions",
    params(("id" = String, Path, description = "Session id")),
    request_body = CreateAnswers,
    responses(
        (status = 202, body = Run),
        (status = 400, response = ErrorResponse),
        (status = 404, response = ErrorResponse),
        (status = 409, description = "The session is not waiting for an answer", body = ErrorResponse),
    ),
)]
pub async fn create_answers(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
    Json(body): Json<CreateAnswers>,
) -> Result<(StatusCode, Json<Run>), ApiError> {
    let session = find_session(&state, workspace, &id).await?;
    let pending = match (&session.status, &session.pending_question) {
        (SessionStatus::NeedsInput, Some(pending)) => {
            serde_json::from_value::<PendingQuestion>(pending.clone())
                .map_err(|error| ApiError::Internal(error.into()))?
        }
        _ => {
            return Err(ApiError::Conflict(
                "this session is not waiting for an answer".into(),
            ));
        }
    };
    let answers = validate_answers(&pending.questions, body.answers)?;
    let item = question::answer_item(&pending.call_id, &pending.questions, &answers);
    let input = history::to_json(&[item])?;
    runs::start(&state, workspace, &session, input).await?;
    Ok((StatusCode::ACCEPTED, Json(run_object(&session, false))))
}

pub(super) fn validate_answers(
    questions: &[question::Question],
    answers: Vec<AnswerInput>,
) -> Result<Vec<Answer>, ApiError> {
    if answers.len() > questions.len() {
        return Err(ApiError::invalid(
            format!("expected at most {} answers", questions.len()),
            "answers",
        ));
    }
    answers
        .into_iter()
        .zip(questions)
        .map(|(answer, question)| {
            for label in &answer.selected {
                if !question.options.iter().any(|option| &option.label == label) {
                    return Err(ApiError::invalid(
                        format!("{label:?} is not an option for {:?}", question.question),
                        "answers",
                    ));
                }
            }
            if !question.multi_select && answer.selected.len() > 1 {
                return Err(ApiError::invalid(
                    format!("{:?} allows only one option", question.question),
                    "answers",
                ));
            }
            Ok(Answer {
                selected: answer.selected,
                text: answer.text,
            })
        })
        .collect()
}

impl From<question::Question> for Question {
    fn from(question: question::Question) -> Self {
        Self {
            question: question.question,
            options: question
                .options
                .into_iter()
                .map(|option| QuestionOption {
                    label: option.label,
                    description: option.description,
                })
                .collect(),
            multi_select: question.multi_select,
        }
    }
}

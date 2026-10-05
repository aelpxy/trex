use axum::{Json, extract::Query};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::Admin;
use crate::api::{List, error::ErrorResponse};

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LogsQuery {
    /// Only lines after this sequence number, to follow the log.
    after: Option<u64>,
    /// At most this many of the newest lines, up to 1000.
    #[param(default = 500, minimum = 1, maximum = 1000)]
    limit: Option<usize>,
}

const DEFAULT_LOG_LINES: usize = 500;
const MAX_LOG_LINES: usize = 1000;

#[derive(Serialize, ToSchema)]
pub struct LogLine {
    seq: u64,
    /// Unix milliseconds.
    time: i64,
    #[schema(example = "info")]
    level: String,
    #[schema(example = "trex::runs")]
    target: String,
    message: String,
    /// The event's other fields, as `key=value` pairs.
    fields: String,
}

/// Read recent logs
///
/// This instance's most recent log lines, oldest first. Each instance keeps the last 2000.
#[utoipa::path(
    get,
    operation_id = "list_logs",
    path = "/admin/logs",
    tag = "admin",
    params(LogsQuery),
    responses((status = 200, body = List<LogLine>), (status = 403, response = ErrorResponse)),
)]
pub async fn logs(_: Admin, Query(query): Query<LogsQuery>) -> Json<List<LogLine>> {
    let limit = query
        .limit
        .unwrap_or(DEFAULT_LOG_LINES)
        .clamp(1, MAX_LOG_LINES);
    let data = crate::logging::recent(query.after.unwrap_or(0), limit)
        .into_iter()
        .map(|line| LogLine {
            seq: line.seq,
            time: line.time_ms,
            level: line.level.as_str().to_ascii_lowercase(),
            target: line.target,
            message: line.message,
            fields: line.fields,
        })
        .collect();
    Json(List::new(data, false))
}

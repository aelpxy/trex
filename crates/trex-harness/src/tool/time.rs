use anyhow::Context;
use async_openai::types::responses::FunctionTool;
use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::Tz;
use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Tool, ToolContext};

pub struct CurrentTime;

#[derive(Deserialize)]
struct Args {
    timezone: Option<String>,
}

impl Tool for CurrentTime {
    fn definition(&self) -> FunctionTool {
        FunctionTool {
            name: "get_current_time".into(),
            description: Some(
                "Get the current date and time, in UTC or in an IANA timezone such as Europe/Berlin. \
                 Use it for anything that depends on the time of day, deadlines or elapsed time."
                    .into(),
            ),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "timezone": {"type": ["string", "null"], "description": "IANA timezone name; UTC when null."}
                },
                "required": ["timezone"],
                "additionalProperties": false,
            })),
            strict: Some(true),
            ..Default::default()
        }
    }

    fn call<'a>(
        &'a self,
        _ctx: ToolContext<'a>,
        args: Value,
    ) -> BoxFuture<'a, anyhow::Result<String>> {
        Box::pin(async move {
            let args: Args = serde_json::from_value(args)?;
            describe(Utc::now(), args.timezone.as_deref())
        })
    }
}

fn describe(now: DateTime<Utc>, timezone: Option<&str>) -> anyhow::Result<String> {
    let Some(name) = timezone else {
        return Ok(format_time(&now, "UTC"));
    };
    let zone: Tz = name.parse().ok().with_context(|| {
        format!("unknown timezone {name}; use an IANA name like America/New_York")
    })?;
    Ok(format_time(&now.with_timezone(&zone), name))
}

fn format_time<Z: TimeZone>(time: &DateTime<Z>, zone: &str) -> String
where
    Z::Offset: std::fmt::Display,
{
    format!(
        "{} ({zone}, {}, {})",
        time.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        time.format("%A"),
        time.format("%Z"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_the_time_in_a_timezone() {
        let now = Utc.with_ymd_and_hms(2026, 10, 5, 9, 30, 0).unwrap();
        assert_eq!(
            describe(now, None).unwrap(),
            "2026-10-05T09:30:00+00:00 (UTC, Monday, UTC)"
        );
        assert_eq!(
            describe(now, Some("Europe/Berlin")).unwrap(),
            "2026-10-05T11:30:00+02:00 (Europe/Berlin, Monday, CEST)"
        );
        assert_eq!(
            describe(now, Some("Pacific/Auckland")).unwrap(),
            "2026-10-05T22:30:00+13:00 (Pacific/Auckland, Monday, NZDT)"
        );
        assert!(describe(now, Some("Mars/Olympus")).is_err());
    }
}

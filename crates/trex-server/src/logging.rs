use std::{
    collections::VecDeque,
    fmt::Write,
    sync::{Mutex, OnceLock},
};

use chrono::Utc;
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{EnvFilter, Layer, fmt, layer::Context, prelude::*};

use crate::config::LogFormat;

// recent lines kept for the admin logs view; each instance keeps its own
const KEPT_LINES: usize = 2000;

#[derive(Clone)]
pub struct LogLine {
    pub seq: u64,
    pub time_ms: i64,
    pub level: tracing::Level,
    pub target: String,
    pub message: String,
    pub fields: String,
}

struct Ring {
    next: u64,
    lines: VecDeque<LogLine>,
}

static RECENT: OnceLock<Mutex<Ring>> = OnceLock::new();

fn ring() -> &'static Mutex<Ring> {
    RECENT.get_or_init(|| {
        Mutex::new(Ring {
            next: 1,
            lines: VecDeque::with_capacity(KEPT_LINES),
        })
    })
}

// lines after `after`, oldest first, at most `limit` of the newest
pub fn recent(after: u64, limit: usize) -> Vec<LogLine> {
    let ring = ring().lock().expect("log ring lock poisoned");
    let newer: Vec<&LogLine> = ring.lines.iter().filter(|line| line.seq > after).collect();
    let skip = newer.len().saturating_sub(limit);
    newer.into_iter().skip(skip).cloned().collect()
}

struct RecentLayer;

#[derive(Default)]
struct Fields {
    message: String,
    rest: String,
}

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(
                self.rest,
                "{}{}={value:?}",
                if self.rest.is_empty() { "" } else { " " },
                field.name()
            );
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            let _ = write!(
                self.rest,
                "{}{}={value}",
                if self.rest.is_empty() { "" } else { " " },
                field.name()
            );
        }
    }
}

impl<S: Subscriber> Layer<S> for RecentLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut ring = ring().lock().expect("log ring lock poisoned");
        let seq = ring.next;
        ring.next += 1;
        if ring.lines.len() == KEPT_LINES {
            ring.lines.pop_front();
        }
        ring.lines.push_back(LogLine {
            seq,
            time_ms: Utc::now().timestamp_millis(),
            level: *event.metadata().level(),
            target: event.metadata().target().to_owned(),
            message: fields.message,
            fields: fields.rest,
        });
    }
}

pub fn init(format: LogFormat) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(
            "info,trex=debug,trex_harness=debug,trex_sandbox=debug,trex_store=debug,tower_http=info,sqlx=warn",
        )
    });
    let output = match format {
        LogFormat::Text => fmt::layer().boxed(),
        LogFormat::Json => fmt::layer()
            .json()
            .flatten_event(true)
            .with_current_span(true)
            .with_span_list(false)
            .boxed(),
    };
    tracing_subscriber::registry()
        .with(filter)
        .with(output)
        .with(RecentLayer)
        .init();
}

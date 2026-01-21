use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trace {
    pub id: String,
    pub agent_id: String,
    pub method: String,
    pub url: String,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub duration_ms: Option<u64>,
    pub status_code: Option<u16>,
    pub events: Vec<TraceEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEvent {
    pub timestamp: DateTime<Utc>,
    pub category: EventCategory,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventCategory {
    Info,
    Chaos,
    Payment,
    Upstream,
    Error,
}

impl Trace {
    pub fn new(agent_id: &str, method: &str, url: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            agent_id: agent_id.to_string(),
            method: method.to_string(),
            url: url.to_string(),
            start_time: Utc::now(),
            end_time: None,
            duration_ms: None,
            status_code: None,
            events: Vec::new(),
        }
    }

    pub fn log(&mut self, category: EventCategory, message: &str) {
        self.events.push(TraceEvent {
            timestamp: Utc::now(),
            category,
            message: message.to_string(),
        });
    }

    pub fn finish(&mut self, status: u16) {
        let now = Utc::now();
        self.end_time = Some(now);
        self.duration_ms = Some((now - self.start_time).num_milliseconds() as u64);
        self.status_code = Some(status);
    }
}
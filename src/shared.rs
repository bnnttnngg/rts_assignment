use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LinkMsg {
    Hello { who: String, at: DateTime<Utc> },
    Ack { msg: String, at: DateTime<Utc> },
}
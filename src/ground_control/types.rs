use chrono::{DateTime, Utc};

use crate::satellite::types::{CommandKind, FaultCode};

#[derive(Debug, Clone)]
pub struct ScheduledCommand {
    pub id: u64,
    pub kind: CommandKind,
    pub dispatch_at: DateTime<Utc>,
    pub deadline_ms: u64,
}

#[derive(Debug, Default, Clone)]
pub struct InterlockState {
    pub fault_active: bool,
    pub last_fault: Option<FaultCode>,
}
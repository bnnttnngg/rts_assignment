use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SensorKind {
    Thermal,
    Attitude,
    Power,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryPacket {
    pub seq: u64,
    pub sensor: SensorKind,
    pub priority: u8,
    pub generated_at: DateTime<Utc>,
    pub payload: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandKind {
    SetModeSafe,
    ResetSubsystem,
    AntennaAlign,
    RequestResend { from_seq: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandMsg {
    pub id: u64,
    pub kind: CommandKind,
    pub issued_at: DateTime<Utc>,
    pub deadline_ms: u64, // urgent <=2ms
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FaultCode {
    ThermalMissedCycles,
    DeadlineViolation,
    BufferOver80,
    LossOfContact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultMsg {
    pub code: FaultCode,
    pub detail: String,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LinkMsg {
    Hello { who: String, at: DateTime<Utc> },
    Ack { msg: String, at: DateTime<Utc> },

    Telemetry(TelemetryPacket),
    Command(CommandMsg),
    Fault(FaultMsg),

    Heartbeat { at: DateTime<Utc> },
}
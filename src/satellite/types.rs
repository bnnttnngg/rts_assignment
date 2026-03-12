use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SensorKind {
    Thermal,
    Attitude,
    Power,
}

impl SensorKind {
    pub fn expected_period_ms(&self) -> u64 {
        match self {
            SensorKind::Thermal => 50,
            SensorKind::Attitude => 100,
            SensorKind::Power => 200,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryPacket {
    pub seq: u64,
    pub sensor: SensorKind,
    pub priority: u8, // 0 = highest
    pub generated_at: DateTime<Utc>,
    pub payload: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandKind {
    SetModeSafe,
    ResetSubsystem,
    AntennaAlign,
    RequestResend { from_seq: u64, sensor: SensorKind },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandMsg {
    pub id: u64,
    pub kind: CommandKind,
    pub issued_at: DateTime<Utc>,
    pub deadline_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FaultCode {
    ThermalMissedCycles,
    DownlinkInitMissed,
    BufferOver80,
    LossOfContact,
    RecoveryTooSlow,
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
    CommandAck { id: u64, at: DateTime<Utc> },
    Fault(FaultMsg),

    Heartbeat { at: DateTime<Utc> },
}
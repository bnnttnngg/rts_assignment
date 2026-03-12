use chrono::Utc;
use tracing::warn;

use crate::satellite::types::{FaultCode, FaultMsg, LinkMsg};

pub struct FaultManager {
    thermal_miss_streak: u32,
}

impl FaultManager {
    pub fn new() -> Self {
        Self { thermal_miss_streak: 0 }
    }

    pub fn on_thermal_ok(&mut self) {
        self.thermal_miss_streak = 0;
    }

    pub fn on_thermal_miss(&mut self, detail: String) -> Option<LinkMsg> {
        self.thermal_miss_streak += 1;

        warn!("(SAT) thermal miss streak={}", self.thermal_miss_streak);

        if self.thermal_miss_streak > 3 {
            self.thermal_miss_streak = 0;

            Some(LinkMsg::Fault(FaultMsg {
                code: FaultCode::ThermalMissedCycles,
                detail,
                at: Utc::now(),
            }))
        } else {
            None
        }
    }
}
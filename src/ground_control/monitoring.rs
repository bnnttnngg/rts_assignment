use std::collections::HashMap;
use std::time::{Duration, Instant};

use chrono::Utc;
use tracing::{info, warn};

use crate::satellite::types::{
    CommandKind, CommandMsg, FaultCode, FaultMsg, LinkMsg, SensorKind, TelemetryPacket,
};

fn expected_period_ms(sensor: SensorKind) -> u64 {
    match sensor {
        SensorKind::Thermal => 10,
        SensorKind::Attitude => 20,
        SensorKind::Power => 50,
    }
}

pub struct GcsMonitor {
    next_seq: HashMap<SensorKind, u64>,
    miss_streak: HashMap<SensorKind, u32>,
    last_arrival: HashMap<SensorKind, Instant>,
    max_reception_drift: Duration,
    max_latency_ms: i64,
    last_summary: Instant,
}

impl GcsMonitor {
    pub fn new() -> Self {
        Self {
            next_seq: HashMap::new(),
            miss_streak: HashMap::new(),
            last_arrival: HashMap::new(),
            max_reception_drift: Duration::ZERO,
            max_latency_ms: 0,
            last_summary: Instant::now(),
        }
    }

    pub fn on_telemetry(&mut self, p: &TelemetryPacket) -> (Option<LinkMsg>, Option<LinkMsg>) {
        // packet reception latency
        let lat_ms = (Utc::now() - p.generated_at).num_milliseconds();
        self.max_latency_ms = self.max_latency_ms.max(lat_ms);

        // reception drift = |actual interval - expected interval|
        let now = Instant::now();
        if let Some(prev) = self.last_arrival.insert(p.sensor, now) {
            let actual = now.duration_since(prev);
            let expected = Duration::from_millis(expected_period_ms(p.sensor));
            let drift = if actual > expected {
                actual - expected
            } else {
                expected - actual
            };

            self.max_reception_drift = self.max_reception_drift.max(drift);
            info!("(GCS) RECEPTION drift sensor={:?} drift={:?}", p.sensor, drift);
        }

        let expected_seq = *self.next_seq.get(&p.sensor).unwrap_or(&0);
        let mut request_resend: Option<LinkMsg> = None;
        let mut loss_of_contact: Option<LinkMsg> = None;

        if p.seq > expected_seq {
            let streak = self.miss_streak.entry(p.sensor).or_insert(0);
            *streak += 1;

            warn!(
                "(GCS) MISSING sensor={:?} expected_seq={} got_seq={} miss_streak={}",
                p.sensor, expected_seq, p.seq, *streak
            );

            request_resend = Some(LinkMsg::Command(CommandMsg {
                id: 9000 + p.seq,
                kind: CommandKind::RequestResend {
                    from_seq: expected_seq,
                    sensor: p.sensor,
                },
                issued_at: Utc::now(),
                deadline_ms: 2,
            }));

            if *streak >= 3 {
                warn!("(GCS) LOSS OF CONTACT sensor={:?} (>=3 missing)", p.sensor);

                loss_of_contact = Some(LinkMsg::Fault(FaultMsg {
                    code: FaultCode::LossOfContact,
                    detail: format!("loss of contact sensor {:?}", p.sensor),
                    at: Utc::now(),
                }));
            }
        } else {
            self.miss_streak.insert(p.sensor, 0);
        }

        self.next_seq.insert(p.sensor, p.seq + 1);

        if self.last_summary.elapsed().as_secs() >= 5 {
            info!(
                "(GCS) MONITOR summary max_latency_ms={} max_reception_drift={:?}",
                self.max_latency_ms, self.max_reception_drift
            );
            self.last_summary = Instant::now();
        }

        (request_resend, loss_of_contact)
    }
}
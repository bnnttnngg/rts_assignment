use std::collections::HashMap;
use std::time::{Duration, Instant};

use chrono::Utc;
use tracing::{info, warn};

use crate::satellite::types::{
    CommandKind, CommandMsg, FaultCode, FaultMsg, LinkMsg, SensorKind, TelemetryPacket,
};

#[derive(Debug)]
pub struct GcsMonitor {
    // expected next seq per sensor
    next_seq: HashMap<SensorKind, u64>,
    // consecutive missing streak per sensor
    miss_streak: HashMap<SensorKind, u32>,

    // drift & latency
    last_arrival: HashMap<SensorKind, Instant>,
    pub max_reception_drift: Duration,
    pub max_latency_ms: i64,

    // summary timer
    pub last_summary: Instant,
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
        // 1) latency: now - generated_at
        let lat = (Utc::now() - p.generated_at).num_milliseconds();
        if lat > self.max_latency_ms {
            self.max_latency_ms = lat;
        }

        // 2) reception drift: actual interval vs expected
        let now = Instant::now();
        if let Some(prev) = self.last_arrival.insert(p.sensor, now) {
            let actual = now.duration_since(prev);
            let expected = Duration::from_millis(p.sensor.expected_period_ms());
            let drift = if actual > expected { actual - expected } else { expected - actual };
            if drift > self.max_reception_drift {
                self.max_reception_drift = drift;
            }
        }

        // 3) missing packet detection based on seq
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

            // re-request (kalau enum kamu RequestResend hanya punya from_seq, hapus sensor field)
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
                    detail: format!("loss of contact for {:?}", p.sensor),
                    at: Utc::now(),
                }));
            }
        } else {
            // good packet -> reset streak
            self.miss_streak.insert(p.sensor, 0);
        }

        // next expected
        self.next_seq.insert(p.sensor, p.seq + 1);

        // summary log every 5s
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
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use chrono::Utc;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{info, warn};

use crate::satellite::benchmarking::FaultFlags;
use crate::satellite::types::{LinkMsg, SensorKind, TelemetryPacket};

pub struct BoundedPriorityBuffer {
    cap: usize,
    q: VecDeque<TelemetryPacket>,
}

impl BoundedPriorityBuffer {
    pub fn new(cap: usize) -> Self {
        Self { cap, q: VecDeque::with_capacity(cap) }
    }

    pub fn len(&self) -> usize { self.q.len() }
    pub fn cap(&self) -> usize { self.cap }

    pub fn push(&mut self, pkt: TelemetryPacket) -> Option<TelemetryPacket> {
        if self.q.len() < self.cap {
            self.q.push_back(pkt);
            return None;
        }

        // buffer full: drop lowest priority (largest number)
        let (worst_idx, worst_prio) = self.q.iter()
            .enumerate()
            .map(|(i, p)| (i, p.priority))
            .max_by_key(|x| x.1)
            .unwrap();

        if pkt.priority < worst_prio {
            let dropped = self.q.remove(worst_idx).unwrap();
            self.q.push_back(pkt);
            Some(dropped)
        } else {
            // new pkt is worse → drop new
            Some(pkt)
        }
    }

    pub fn pop(&mut self) -> Option<TelemetryPacket> {
        self.q.pop_front()
    }
}

static SEQ_THERMAL: AtomicU64 = AtomicU64::new(0);
static SEQ_ATT: AtomicU64 = AtomicU64::new(0);
static SEQ_PWR: AtomicU64 = AtomicU64::new(0);

fn next_seq(kind: SensorKind) -> u64 {
    match kind {
        SensorKind::Thermal => SEQ_THERMAL.fetch_add(1, Ordering::Relaxed),
        SensorKind::Attitude => SEQ_ATT.fetch_add(1, Ordering::Relaxed),
        SensorKind::Power => SEQ_PWR.fetch_add(1, Ordering::Relaxed),
    }
}

pub async fn run_sensors(
    mut buf: BoundedPriorityBuffer,
    downlink_tx: mpsc::Sender<TelemetryPacket>,
    gcs_tx: mpsc::Sender<LinkMsg>,
    flags: FaultFlags,
) {
    // config (distinct intervals + priority)
    let sensors = vec![
        (SensorKind::Thermal, Duration::from_millis(10), 0, true),
        (SensorKind::Attitude, Duration::from_millis(20), 1, false),
        (SensorKind::Power, Duration::from_millis(50), 2, false),
    ];

    for (kind, period, prio, critical) in sensors {
        let mut buf_local = BoundedPriorityBuffer { cap: buf.cap(), q: VecDeque::new() };
        // NOTE: biar simpel: setiap sensor punya buffer local kecil lalu forwarding
        // supaya demo gampang. (Kalau mau 1 shared buffer, pakai Mutex/Arc.)

        let dl = downlink_tx.clone();
        let gcs = gcs_tx.clone();
        let flags_cl = FaultFlags { delay: flags.delay.clone(), corrupt: flags.corrupt.clone() };

        tokio::spawn(async move {
            let mut expected_next = Instant::now() + period;
            loop {
                let now = Instant::now();

                // jitter = |actual - expected|
                let jitter = if now > expected_next { now - expected_next } else { expected_next - now };
                if critical && jitter > Duration::from_millis(1) {
                    warn!("(SAT) JITTER VIOLATION sensor={:?} jitter={:?} (>1ms)", kind, jitter);
                }

                let read_instant = Instant::now();
                let mut payload = format!("sensor={:?} value=OK", kind);

                // fault injection effect
                if flags_cl.delay.load(Ordering::Relaxed) {
                    sleep(Duration::from_millis(5)).await;
                }
                if flags_cl.corrupt.load(Ordering::Relaxed) {
                    payload = "CORRUPTED_PAYLOAD".into();
                }

                let pkt = TelemetryPacket {
                    seq: next_seq(kind),
                    sensor: kind,
                    priority: prio,
                    generated_at: Utc::now(),
                    payload,
                };

                // bounded buffer insert + latency
                let dropped = buf_local.push(pkt.clone());
                let insert_lat = read_instant.elapsed();
                info!("(SAT) sensor={:?} insert_latency={:?}", kind, insert_lat);

                if let Some(d) = dropped {
                    warn!("(SAT) BUFFER DROP sensor={:?} seq={} at={}", d.sensor, d.seq, Utc::now());
                }

                // forward to downlink + also to GCS as telemetry
                let _ = dl.send(pkt.clone()).await;
                let _ = gcs.send(LinkMsg::Telemetry(pkt)).await;

                expected_next += period;
                sleep(period).await;
            }
        });
    }
}
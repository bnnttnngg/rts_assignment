use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use chrono::Utc;
use tokio::time::{interval, MissedTickBehavior};
use tracing::{info, warn};

use crate::satellite::benchmarking::FaultFlags;
use crate::satellite::types::{SensorKind, TelemetryPacket};

pub struct SharedBuffer {
    cap: usize,
    q: VecDeque<TelemetryPacket>,
}

impl SharedBuffer {
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            q: VecDeque::with_capacity(cap),
        }
    }

    pub fn len(&self) -> usize {
        self.q.len()
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    pub fn push_prioritized(&mut self, pkt: TelemetryPacket) -> Option<TelemetryPacket> {
        if self.q.len() < self.cap {
            self.q.push_back(pkt);
            return None;
        }

        let (worst_i, worst_p) = self
            .q
            .iter()
            .enumerate()
            .map(|(i, p)| (i, p.priority))
            .max_by_key(|x| x.1)
            .unwrap();

        if pkt.priority < worst_p {
            let dropped = self.q.remove(worst_i).unwrap();
            self.q.push_back(pkt);
            Some(dropped)
        } else {
            Some(pkt)
        }
    }

    pub fn pop(&mut self) -> Option<TelemetryPacket> {
        self.q.pop_front()
    }
}

static SEQ_TH: AtomicU64 = AtomicU64::new(0);
static SEQ_AT: AtomicU64 = AtomicU64::new(0);
static SEQ_PW: AtomicU64 = AtomicU64::new(0);

fn next_seq(k: SensorKind) -> u64 {
    match k {
        SensorKind::Thermal => SEQ_TH.fetch_add(1, Ordering::Relaxed),
        SensorKind::Attitude => SEQ_AT.fetch_add(1, Ordering::Relaxed),
        SensorKind::Power => SEQ_PW.fetch_add(1, Ordering::Relaxed),
    }
}

pub fn spawn_sensors(buffer: Arc<Mutex<SharedBuffer>>, flags: FaultFlags) {
    spawn_one(
        buffer.clone(),
        flags.clone(),
        SensorKind::Thermal,
        Duration::from_millis(10),
        0,
        true,
    );

    spawn_one(
        buffer.clone(),
        flags.clone(),
        SensorKind::Attitude,
        Duration::from_millis(20),
        1,
        false,
    );

    spawn_one(
        buffer.clone(),
        flags.clone(),
        SensorKind::Power,
        Duration::from_millis(50),
        2,
        false,
    );
}

fn spawn_one(
    buffer: Arc<Mutex<SharedBuffer>>,
    flags: FaultFlags,
    kind: SensorKind,
    period: Duration,
    prio: u8,
    critical: bool,
) {
    tokio::spawn(async move {
        let mut itv = interval(period);
        itv.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let mut last = Instant::now();
        let mut count: u64 = 0;

        loop {
            itv.tick().await;

            let now = Instant::now();
            let actual = now.duration_since(last);
            last = now;

            let jitter = if actual > period {
                actual - period
            } else {
                period - actual
            };

            if critical && jitter > Duration::from_millis(1) {
                warn!("(SAT) JITTER VIOLATION sensor={:?} jitter={:?}", kind, jitter);
            }

            let read_t0 = Instant::now();

            if flags.delay.load(Ordering::Relaxed) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }

            count += 1;

            if kind == SensorKind::Attitude && count % 40 == 0 {
                warn!("(SAT) SIMULATE DROP sensor={:?}", kind);
                continue;
            }

            let payload = if flags.corrupt.load(Ordering::Relaxed) {
                "CORRUPTED".to_string()
            } else {
                format!("OK sensor={:?}", kind)
            };

            let pkt = TelemetryPacket {
                seq: next_seq(kind),
                sensor: kind,
                priority: prio,
                generated_at: Utc::now(),
                payload,
            };

            let insert_latency = read_t0.elapsed();

            let dropped = {
                let mut b = buffer.lock().unwrap();

                let d = b.push_prioritized(pkt.clone());

                let fill = (b.len() as f64 / b.cap() as f64) * 100.0;

                if fill > 80.0 {
                    warn!("(SAT) BUFFER FILL {:.1}% (>80%)", fill);
                }

                d
            };

            if count % 25 == 0 {
                info!("(SAT) SENSOR {:?} insert_latency={:?}", kind, insert_latency);
            }

            if let Some(d) = dropped {
                warn!(
                    "(SAT) BUFFER DROP sensor={:?} seq={} at={}",
                    d.sensor,
                    d.seq,
                    Utc::now()
                );
            }
        }
    });
}
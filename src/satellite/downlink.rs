use std::collections::VecDeque;
use std::time::{Duration, Instant};

use chrono::Utc;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{info, warn};

use crate::satellite::types::{FaultCode, FaultMsg, LinkMsg, TelemetryPacket};

pub async fn run_downlink(
    mut rx: mpsc::Receiver<TelemetryPacket>,
    gcs_tx: mpsc::Sender<LinkMsg>,
) {
    let mut queue: VecDeque<(TelemetryPacket, Instant)> = VecDeque::new();
    let capacity = 100;

    loop {
        while let Ok(pkt) = rx.try_recv() {
            if queue.len() >= capacity {
                warn!("(SAT) DOWNLINK QUEUE FULL drop seq={}", pkt.seq);
                continue;
            }

            queue.push_back((pkt, Instant::now()));

            let fill = (queue.len() as f64 / capacity as f64) * 100.0;

            if fill > 80.0 {
                warn!("(SAT) DEGRADED MODE queue_fill={:.1}%", fill);

                let _ = gcs_tx.send(LinkMsg::Fault(FaultMsg {
                    code: FaultCode::BufferOver80,
                    detail: format!("queue fill {:.1}%", fill),
                    at: Utc::now(),
                })).await;
            }
        }

        let window_start = Instant::now();

        info!("(SAT) VISIBILITY WINDOW OPEN");

        if window_start.elapsed() > Duration::from_millis(5) {
            warn!("(SAT) DOWNLINK INIT MISSED");
        }

        if let Some((pkt, enq)) = queue.pop_front() {
            let latency = enq.elapsed();

            info!(
                "(SAT) DOWNLINK SENT seq={} sensor={:?} latency={:?}",
                pkt.seq, pkt.sensor, latency
            );

            let _ = gcs_tx.send(LinkMsg::Telemetry(pkt)).await;
        }

        sleep(Duration::from_secs(5)).await;
    }
}
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{info, warn};

use crate::satellite::sensors::SharedBuffer;
use crate::satellite::types::{FaultCode, FaultMsg, LinkMsg};

pub async fn run_downlink(buffer: Arc<Mutex<SharedBuffer>>, tx: mpsc::Sender<LinkMsg>) {
    let mut window_count: u64 = 0;

    loop {
        // window every 100ms so telemetry stream stays realistic
        sleep(Duration::from_millis(100)).await;
        window_count += 1;

        let win_open = Instant::now();
        info!("(SAT) VISIBILITY WINDOW OPEN");

        // simulate missed init once in a while
        if window_count % 20 == 0 {
            tokio::time::sleep(Duration::from_millis(6)).await;
        }

        let init_ms = win_open.elapsed().as_millis();
        if init_ms > 5 {
            warn!("(SAT) DOWNLINK INIT MISSED init_ms={} (>5ms)", init_ms);
            let _ = tx.send(LinkMsg::Fault(FaultMsg {
                code: FaultCode::DownlinkInitMissed,
                detail: format!("init_ms={}", init_ms),
                at: Utc::now(),
            })).await;
        }

        let mut sent = 0usize;
        let mut first_send_done = false;

        while win_open.elapsed() <= Duration::from_millis(30) {
            let pkt_opt = {
                let mut b = buffer.lock().unwrap();
                b.pop()
            };

            match pkt_opt {
                Some(pkt) => {
                    if !first_send_done {
                        let prep_ms = win_open.elapsed().as_millis();
                        if prep_ms > 30 {
                            warn!("(SAT) DOWNLINK PREPARE MISSED prep_ms={} (>30ms)", prep_ms);
                        } else {
                            info!("(SAT) DOWNLINK PREPARED within {}ms", prep_ms);
                        }
                        first_send_done = true;
                    }

                    let queue_latency_ms = (Utc::now() - pkt.generated_at).num_milliseconds();
                    let compressed_len = (pkt.payload.len() / 2).max(1);

                    info!(
                        "(SAT) DOWNLINK packetized seq={} sensor={:?} queue_latency_ms={} compressed_len={}",
                        pkt.seq, pkt.sensor, queue_latency_ms, compressed_len
                    );

                    let _ = tx.send(LinkMsg::Telemetry(pkt)).await;
                    sent += 1;
                }
                None => {
                    tokio::task::yield_now().await;
                }
            }
        }

        info!("(SAT) DOWNLINK WINDOW SENT {} packet(s)", sent);
    }
}
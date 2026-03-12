use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use tokio::time::sleep;
use tracing::{info, warn};

use crate::satellite::core::TcpLink;
use crate::satellite::sensors::SharedBuffer;
use crate::satellite::types::{FaultCode, FaultMsg, LinkMsg};

pub async fn run_downlink(buffer: Arc<Mutex<SharedBuffer>>, mut link: TcpLink) {
    loop {
        sleep(Duration::from_secs(5)).await;

        let win_open = Instant::now();
        info!("(SAT) VISIBILITY WINDOW OPEN");

        let init_start = Instant::now();
        let init_ms = init_start.elapsed().as_millis();

        if init_ms > 5 {
            warn!("(SAT) DOWNLINK INIT MISSED init_ms={} (>5ms)", init_ms);

            let _ = link
                .tx
                .send(LinkMsg::Fault(FaultMsg {
                    code: FaultCode::DownlinkInitMissed,
                    detail: format!("init_ms={}", init_ms),
                    at: Utc::now(),
                }))
                .await;
        }

        let mut sent_any = false;

        while win_open.elapsed() <= Duration::from_millis(30) {
            let pkt_opt = {
                let mut b = buffer.lock().unwrap();
                b.pop()
            };

            if let Some(pkt) = pkt_opt {
                sent_any = true;

                let _ = link.tx.send(LinkMsg::Telemetry(pkt)).await;

                info!("(SAT) DOWNLINK SENT within 30ms");

                break;
            } else {
                tokio::task::yield_now().await;
            }
        }

        if !sent_any {
            warn!("(SAT) DOWNLINK PREPARE MISSED (>30ms no data)");
        }
    }
}
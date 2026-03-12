use std::time::Instant;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::satellite::types::LinkMsg;

pub struct TcpLink {
    pub rx: mpsc::Receiver<LinkMsg>,
    pub tx: mpsc::Sender<LinkMsg>,
}

async fn reader_task(read_half: tokio::io::ReadHalf<TcpStream>, tx_to_core: mpsc::Sender<LinkMsg>) {
    let mut lines = BufReader::new(read_half).lines();

    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                let s = line.trim();
                if s.is_empty() {
                    continue;
                }

                // ✅ decode timing (<=3ms)
                let t0 = Instant::now();
                match serde_json::from_str::<LinkMsg>(s) {
                    Ok(msg) => {
                        let decode_ms = t0.elapsed().as_millis();
                        if decode_ms > 3 {
                            warn!("(GCS) DECODE SLOW decode_ms={} (>3ms)", decode_ms);
                        } else {
                            info!("(GCS) decode_ms={}", decode_ms);
                        }

                        if tx_to_core.send(msg).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => warn!("(GCS) bad JSON ignored: {}", e),
                }
            }
            Ok(None) => {
                warn!("(GCS) peer disconnected");
                break;
            }
            Err(e) => {
                error!("(GCS) read error: {}", e);
                break;
            }
        }
    }
}

async fn writer_task(
    mut write_half: tokio::io::WriteHalf<TcpStream>,
    mut rx_from_core: mpsc::Receiver<LinkMsg>,
) {
    while let Some(msg) = rx_from_core.recv().await {
        let mut s = match serde_json::to_string(&msg) {
            Ok(v) => v,
            Err(e) => {
                warn!("(GCS) serialize failed: {}", e);
                continue;
            }
        };
        s.push('\n');

        if let Err(e) = write_half.write_all(s.as_bytes()).await {
            error!("(GCS) write failed: {}", e);
            break;
        }
        let _ = write_half.flush().await;
    }
}

pub async fn start_server(bind: &str) -> Result<TcpLink, String> {
    let listener = TcpListener::bind(bind)
        .await
        .map_err(|e| format!("bind failed: {e}"))?;

    info!("GCS listening on {}", bind);

    let (stream, addr) = listener
        .accept()
        .await
        .map_err(|e| format!("accept failed: {e}"))?;

    stream
        .set_nodelay(true)
        .map_err(|e| format!("set_nodelay failed: {e}"))?;

    info!("SAT connected from {}", addr);

    let (r, w) = tokio::io::split(stream);

    let (to_core_tx, to_core_rx) = mpsc::channel::<LinkMsg>(4096);
    let (to_net_tx, to_net_rx) = mpsc::channel::<LinkMsg>(4096);

    tokio::spawn(reader_task(r, to_core_tx));
    tokio::spawn(writer_task(w, to_net_rx));

    Ok(TcpLink {
        rx: to_core_rx,
        tx: to_net_tx,
    })
}
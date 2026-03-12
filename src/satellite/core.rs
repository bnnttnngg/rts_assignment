use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
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
                if s.is_empty() { continue; }
                match serde_json::from_str::<LinkMsg>(s) {
                    Ok(msg) => { let _ = tx_to_core.send(msg).await; }
                    Err(e) => warn!("SAT bad JSON ignored: {}", e),
                }
            }
            Ok(None) => { warn!("SAT peer disconnected"); break; }
            Err(e) => { error!("SAT read error: {}", e); break; }
        }
    }
}

async fn writer_task(mut write_half: tokio::io::WriteHalf<TcpStream>, mut rx_from_core: mpsc::Receiver<LinkMsg>) {
    while let Some(msg) = rx_from_core.recv().await {
        let mut s = match serde_json::to_string(&msg) {
            Ok(v) => v,
            Err(e) => { warn!("SAT serialize failed: {}", e); continue; }
        };
        s.push('\n');
        if let Err(e) = write_half.write_all(s.as_bytes()).await {
            error!("SAT write failed: {}", e);
            break;
        }
        let _ = write_half.flush().await;
    }
}

pub async fn connect_server(connect: &str) -> Result<TcpLink, String> {
    let stream = TcpStream::connect(connect).await.map_err(|e| format!("connect failed: {e}"))?;
    stream.set_nodelay(true).map_err(|e| format!("set_nodelay failed: {e}"))?;
    info!("SAT connected to {}", connect);

    let (r, w) = tokio::io::split(stream);
    let (to_core_tx, to_core_rx) = mpsc::channel::<LinkMsg>(2048);
    let (to_net_tx, to_net_rx) = mpsc::channel::<LinkMsg>(2048);

    tokio::spawn(reader_task(r, to_core_tx));
    tokio::spawn(writer_task(w, to_net_rx));

    Ok(TcpLink { rx: to_core_rx, tx: to_net_tx })
}
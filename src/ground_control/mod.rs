use chrono::Utc;
use tracing::info;

use crate::net::accept_server;
use crate::shared::LinkMsg;

pub async fn run_gcs(bind: &str) -> Result<(), String> {
    let mut link = accept_server(bind).await?;

    while let Some(msg) = link.rx.recv().await {
        info!("GCS RX: {:?}", msg);

        if let LinkMsg::Hello { who, .. } = msg {
            let reply = LinkMsg::Ack {
                msg: format!("Hello received from {}", who),
                at: Utc::now(),
            };
            let _ = link.tx.send(reply).await;
        }
    }

    Err("GCS: connection closed".into())
}
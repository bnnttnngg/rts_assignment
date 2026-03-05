use chrono::Utc;
use tracing::info;

use crate::net::connect_client;
use crate::shared::LinkMsg;

pub async fn run_sat(connect: &str) -> Result<(), String> {
    let mut link = connect_client(connect).await?;

    let hello = LinkMsg::Hello {
        who: "Student A (SAT)".into(),
        at: Utc::now(),
    };

    link.tx
        .send(hello)
        .await
        .map_err(|_| "SAT: send failed".to_string())?;

    if let Some(msg) = link.rx.recv().await {
        info!("SAT RX: {:?}", msg);
        Ok(())
    } else {
        Err("SAT: no reply".into())
    }
}
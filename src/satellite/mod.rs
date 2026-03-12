pub mod benchmarking;
pub mod core;
pub mod downlink;
pub mod health;
pub mod scheduling;
pub mod sensors;
pub mod types;

use std::sync::{Arc, Mutex};

use crate::satellite::benchmarking::{run_fault_injector, FaultFlags};
use crate::satellite::core::connect_server;
use crate::satellite::sensors::{spawn_sensors, SharedBuffer};

pub async fn start_satellite(connect: &str) -> Result<(), String> {
    let link = connect_server(connect).await?;

    let buffer = Arc::new(Mutex::new(SharedBuffer::new(50)));

    let flags = FaultFlags::new();

    spawn_sensors(buffer.clone(), flags.clone());

    tokio::spawn(run_fault_injector(flags));

    let (dummy_tx, dummy_rx) = tokio::sync::mpsc::channel(1);

    let downlink_link = crate::satellite::core::TcpLink {
        rx: dummy_rx,
        tx: link.tx.clone(),
    };

    tokio::spawn(async move {
        downlink::run_downlink(buffer, downlink_link).await;
    });

    scheduling::run_sat_scheduler_22(link).await
}
pub mod benchmarking;
pub mod core;
pub mod downlink;
pub mod health;
pub mod scheduling;
pub mod sensors;
pub mod types;

use std::sync::{Arc, Mutex};

use crate::satellite::benchmarking::{FaultFlags, run_fault_injector};
use crate::satellite::core::connect_server;
use crate::satellite::sensors::{SharedBuffer, spawn_sensors};

pub async fn start_satellite(connect: &str) -> Result<(), String> {
    let link = connect_server(connect).await?;

    let buffer = Arc::new(Mutex::new(SharedBuffer::new(300)));
    let flags = FaultFlags::new();

    spawn_sensors(buffer.clone(), flags.clone());
    tokio::spawn(run_fault_injector(flags));

    let tx_for_downlink = link.tx.clone();
    tokio::spawn(async move {
        downlink::run_downlink(buffer, tx_for_downlink).await;
    });

    scheduling::run_sat_scheduler_22(link).await
}
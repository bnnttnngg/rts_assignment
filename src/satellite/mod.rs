pub mod benchmarking;
pub mod core;
pub mod downlink;
pub mod health;
pub mod scheduling;
pub mod sensors;
pub mod types;

use crate::satellite::core::connect_server;

pub async fn start_satellite(connect: &str) -> Result<(), String> {
    let link = connect_server(connect).await?;
    scheduling::run_sat_scheduler_22(link).await
}
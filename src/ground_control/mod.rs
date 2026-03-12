pub mod commands;
pub mod core;
pub mod monitoring;
pub mod types;

use crate::ground_control::core::start_server;

pub async fn start_ground_control(bind: &str) -> Result<(), String> {
    let link = start_server(bind).await?;
    commands::run_commands_22(link).await
}
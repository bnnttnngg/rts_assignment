use clap::Parser;
use tracing::{error, info};

#[derive(Debug, Parser)]
struct Args {
    #[arg(long)]
    role: String,

    #[arg(long, default_value = "127.0.0.1:9000")]
    bind: String,

    #[arg(long, default_value = "127.0.0.1:9000")]
    connect: String,
}

fn init_log() {
    // simple logger (no file) to avoid extra errors
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();
}

#[tokio::main]
async fn main() {
    init_log();
    let args = Args::parse();
    info!("Starting role={}", args.role);

    let res = match args.role.to_lowercase().as_str() {
        "gcs" => rts_assignment::ground_control::run_gcs(&args.bind).await,
        "sat" => rts_assignment::satellite::run_sat(&args.connect).await,
        _ => Err("role must be gcs or sat".into()),
    };

    if let Err(e) = res {
        error!("Fatal: {}", e);
        std::process::exit(1);
    }
}
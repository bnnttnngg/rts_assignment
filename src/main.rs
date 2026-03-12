use clap::Parser;
use tracing::{error, info};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[derive(Debug, Parser)]
struct Args {
    #[arg(long)]
    role: String,

    #[arg(long, default_value = "0.0.0.0:9000")]
    bind: String,

    #[arg(long, default_value = "127.0.0.1:9000")]
    connect: String,
}

fn init_log() {
    let env = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let layer = fmt::layer().with_timer(fmt::time::UtcTime::rfc_3339());
    tracing_subscriber::registry().with(env).with(layer).init();
}

#[tokio::main]
async fn main() {
    init_log();
    let args = Args::parse();
    info!("Starting role={}", args.role);

    let res = match args.role.to_lowercase().as_str() {
        "gcs" => rts_assignment::ground_control::start_ground_control(&args.bind).await,
        "sat" => rts_assignment::satellite::start_satellite(&args.connect).await,
        _ => Err("role must be gcs or sat".into()),
    };

    if let Err(e) = res {
        error!("Fatal: {}", e);
        std::process::exit(1);
    }
}
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use tokio::time::{sleep, Duration, Instant};
use tracing::{info, warn};

#[derive(Clone)]
pub struct FaultFlags {
    pub delay: Arc<AtomicBool>,
    pub corrupt: Arc<AtomicBool>,
}

impl FaultFlags {
    pub fn new() -> Self {
        Self {
            delay: Arc::new(AtomicBool::new(false)),
            corrupt: Arc::new(AtomicBool::new(false)),
        }
    }
}

pub async fn run_fault_injector(flags: FaultFlags) {
    let secs = std::env::var("FAULT_PERIOD_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);

    let period = Duration::from_secs(secs);

    loop {
        sleep(period).await;

        info!("(SAT) FAULT INJECT start (every {}s)", secs);

        flags.delay.store(true, Ordering::Relaxed);
        flags.corrupt.store(true, Ordering::Relaxed);

        let t0 = Instant::now();

        sleep(Duration::from_millis(150)).await;

        flags.delay.store(false, Ordering::Relaxed);
        flags.corrupt.store(false, Ordering::Relaxed);

        let rec = t0.elapsed();

        info!("(SAT) FAULT RECOVERED in {:?}", rec);

        if rec > Duration::from_millis(200) {
            warn!("(SAT) MISSION ABORT (recovery >200ms) {:?}", rec);
        }
    }
}
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use tokio::time::{sleep, Duration, Instant};
use tracing::{info, warn};

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
    let period = Duration::from_secs(60);

    loop {
        sleep(period).await;

        info!("(SAT) FAULT INJECT START");

        flags.delay.store(true, Ordering::Relaxed);
        flags.corrupt.store(true, Ordering::Relaxed);

        let start = Instant::now();

        sleep(Duration::from_millis(150)).await;

        flags.delay.store(false, Ordering::Relaxed);
        flags.corrupt.store(false, Ordering::Relaxed);

        let recovery = start.elapsed();

        info!("(SAT) FAULT RECOVERED {:?}", recovery);

        if recovery > Duration::from_millis(200) {
            warn!("(SAT) RECOVERY TOO SLOW {:?}", recovery);
        }
    }
}
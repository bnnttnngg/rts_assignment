use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{info, warn};

use crate::satellite::core::TcpLink;
use crate::satellite::health::FaultManager;
use crate::satellite::types::LinkMsg;

#[derive(Clone)]
struct TaskSpec {
    name: &'static str,
    period: Duration,
    deadline: Duration,
    exec_budget: Duration,
    critical: bool,
}

#[derive(Clone)]
struct ReadyJob {
    spec: TaskSpec,
    expected_release: Instant,
    abs_deadline: Instant,
    remaining: Duration,
}

impl Eq for ReadyJob {}
impl PartialEq for ReadyJob {
    fn eq(&self, other: &Self) -> bool { self.abs_deadline.eq(&other.abs_deadline) }
}

impl Ord for ReadyJob {
    fn cmp(&self, other: &Self) -> Ordering {
        // critical always highest
        if self.spec.critical && !other.spec.critical { return Ordering::Greater; }
        if !self.spec.critical && other.spec.critical { return Ordering::Less; }
        // RM: shorter period higher priority
        other.spec.period.cmp(&self.spec.period)
    }
}
impl PartialOrd for ReadyJob {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

pub async fn run_sat_scheduler_22(mut link: TcpLink) -> Result<(), String> {
    // Budget ditune biar gak backlog gila (tetap ada miss sesekali untuk bukti log)
    let tasks = vec![
        TaskSpec { name: "thermal_control", period: Duration::from_millis(10),  deadline: Duration::from_millis(10),  exec_budget: Duration::from_millis(1), critical: true  },
        TaskSpec { name: "data_compression", period: Duration::from_millis(20), deadline: Duration::from_millis(20), exec_budget: Duration::from_millis(1), critical: false },
        TaskSpec { name: "health_monitor",  period: Duration::from_millis(50), deadline: Duration::from_millis(50), exec_budget: Duration::from_millis(1), critical: false },
        TaskSpec { name: "antenna_align",   period: Duration::from_millis(100),deadline: Duration::from_millis(100),exec_budget: Duration::from_millis(1), critical: false },
    ];

    let start = Instant::now();
    let mut next_release = tasks.iter().map(|t| start + t.period).collect::<Vec<_>>();

    let mut heap = BinaryHeap::<ReadyJob>::new();
    let quantum = Duration::from_millis(1); // windows-friendly

    let mut fm = FaultManager::new();

    let mut miss_count: u64 = 0;
    let mut max_start_delay = Duration::ZERO;
    let mut max_release_drift = Duration::ZERO;

    // CPU util
    let mut active = Duration::ZERO;
    let mut idle = Duration::ZERO;

    let mut last_summary = Instant::now();

    loop {
        // receive commands
        while let Ok(msg) = link.rx.try_recv() {
            if let LinkMsg::Command(c) = msg {
                info!("(SAT) COMMAND RX id={} kind={:?}", c.id, c.kind);
            }
        }

        let now = Instant::now();

        // release jobs
        for (i, spec) in tasks.iter().enumerate() {
            if now >= next_release[i] {
                // drift = now - expected release
                let drift = now.saturating_duration_since(next_release[i]);
                max_release_drift = max_release_drift.max(drift);

                let expected = next_release[i];
                let deadline = expected + spec.deadline;

                heap.push(ReadyJob {
                    spec: spec.clone(),
                    expected_release: expected,
                    abs_deadline: deadline,
                    remaining: spec.exec_budget,
                });

                next_release[i] += spec.period;
            }
        }

        // idle
        if heap.is_empty() {
            let t0 = Instant::now();
            sleep(Duration::from_millis(1)).await;
            idle += t0.elapsed();
        } else {
            // pick job
            let mut job = heap.pop().unwrap();
            let start_exec = Instant::now();

            // start delay / jitter
            let start_delay = start_exec.saturating_duration_since(job.expected_release);
            max_start_delay = max_start_delay.max(start_delay);

            // drop job kalau udah kelewatan jauh (biar queue gak jadi 80 detik)
            if start_exec > job.abs_deadline + Duration::from_millis(200) {
                miss_count += 1;
                warn!("(SAT) DROP overdue job task={} start_delay={:?}", job.spec.name, start_delay);

                if job.spec.critical {
                    if let Some(fmsg) = fm.on_thermal_miss(format!("thermal overdue drop {:?}", start_delay)) {
                        let _ = link.tx.send(fmsg).await;
                    }
                }
                continue;
            }

            // execute slices with preemption check
            let active_start = Instant::now();
            while job.remaining > Duration::ZERO {
                let slice = quantum.min(job.remaining);
                sleep(slice).await;
                job.remaining = job.remaining.saturating_sub(slice);

                if let Some(top) = heap.peek() {
                    if top > &job {
                        heap.push(job);
                        job = heap.pop().unwrap();
                    }
                }
            }
            active += active_start.elapsed();

            // deadline miss
            let end_exec = Instant::now();
            if end_exec > job.abs_deadline {
                miss_count += 1;
                let late_by = end_exec - job.abs_deadline;
                warn!("(SAT) DEADLINE MISS task={} late_by={:?}", job.spec.name, late_by);

                if job.spec.critical {
                    if let Some(fmsg) = fm.on_thermal_miss(format!("thermal missed by {:?}", late_by)) {
                        let _ = link.tx.send(fmsg).await;
                    }
                }
            } else if job.spec.critical {
                fm.on_thermal_ok();
            }
        }

        // summary tiap 5 detik (bukan spam)
        if last_summary.elapsed() >= Duration::from_secs(5) {
            let total = active + idle;
            let cpu = if total.as_millis() == 0 { 0.0 } else { (active.as_secs_f64() / total.as_secs_f64()) * 100.0 };

            info!(
                "(SAT) summary miss={} max_start_delay={:?} max_release_drift={:?} cpu_active={:.1}%",
                miss_count, max_start_delay, max_release_drift, cpu
            );
            last_summary = Instant::now();
            active = Duration::ZERO;
            idle = Duration::ZERO;
        }
    }
}
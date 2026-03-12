use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::time::{Duration, Instant};

use chrono::Utc;
use tokio::time::sleep;
use tracing::{info, warn};

use crate::satellite::core::TcpLink;
use crate::satellite::types::{FaultCode, FaultMsg, LinkMsg};

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
    released_at: Instant,
    abs_deadline: Instant,
    remaining: Duration,
}

impl Eq for ReadyJob {}
impl PartialEq for ReadyJob {
    fn eq(&self, other: &Self) -> bool { self.abs_deadline.eq(&other.abs_deadline) }
}

impl Ord for ReadyJob {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.spec.critical && !other.spec.critical { return Ordering::Greater; }
        if !self.spec.critical && other.spec.critical { return Ordering::Less; }
        other.spec.period.cmp(&self.spec.period)
    }
}
impl PartialOrd for ReadyJob {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

pub async fn run_sat_scheduler_22(mut link: TcpLink) -> Result<(), String> {
    let tasks = vec![
        TaskSpec { name: "thermal_control", period: Duration::from_millis(10),  deadline: Duration::from_millis(10),  exec_budget: Duration::from_millis(1), critical: true  },
        TaskSpec { name: "data_compression", period: Duration::from_millis(20), deadline: Duration::from_millis(20), exec_budget: Duration::from_millis(3), critical: false },
        TaskSpec { name: "health_monitor",  period: Duration::from_millis(50), deadline: Duration::from_millis(50), exec_budget: Duration::from_millis(2), critical: false },
        TaskSpec { name: "antenna_align",   period: Duration::from_millis(100),deadline: Duration::from_millis(100),exec_budget: Duration::from_millis(4), critical: false },
    ];

    let start = Instant::now();
    let mut next_release = tasks.iter().map(|t| start + t.period).collect::<Vec<_>>();
    let mut heap = BinaryHeap::<ReadyJob>::new();
    let quantum = Duration::from_micros(200);

    let mut releases: u64 = 0;
    let mut deadline_miss: u64 = 0;
    let mut max_jitter = Duration::ZERO;
    let mut max_drift = Duration::ZERO;

    loop {
        while let Ok(msg) = link.rx.try_recv() {
            if let LinkMsg::Command(c) = msg {
                info!("(SAT) COMMAND RX id={} kind={:?} deadline_ms={}", c.id, c.kind, c.deadline_ms);
            }
        }

        let now = Instant::now();

        for (i, spec) in tasks.iter().enumerate() {
            if now >= next_release[i] {
                releases += 1;

                let drift = now.saturating_duration_since(next_release[i]);
                max_drift = max_drift.max(drift);

                heap.push(ReadyJob {
                    spec: spec.clone(),
                    released_at: now,
                    abs_deadline: now + spec.deadline,
                    remaining: spec.exec_budget,
                });

                next_release[i] += spec.period;
            }
        }

        let Some(mut job) = heap.pop() else {
            sleep(Duration::from_millis(1)).await;
            continue;
        };

        let start_exec = Instant::now();
        let jitter = start_exec.saturating_duration_since(job.released_at);
        max_jitter = max_jitter.max(jitter);

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

        let end_exec = Instant::now();
        if end_exec > job.abs_deadline {
            deadline_miss += 1;
            let late_by = end_exec - job.abs_deadline;
            warn!("(SAT) DEADLINE MISS task={} late_by={:?}", job.spec.name, late_by);

            if job.spec.critical {
                let _ = link.tx.send(LinkMsg::Fault(FaultMsg {
                    code: FaultCode::ThermalMissedCycles,
                    detail: format!("thermal missed by {:?}", late_by),
                    at: Utc::now(),
                })).await;
            }
        }

        if start.elapsed().as_secs() % 5 == 0 {
            info!(
                "(SAT) summary releases={} miss={} max_jitter={:?} max_drift={:?}",
                releases, deadline_miss, max_jitter, max_drift
            );
        }
    }
}
use std::collections::HashMap;

use chrono::{Duration as ChronoDur, Utc};
use tokio::time::{sleep_until, Duration, Instant};
use tracing::{info, warn};

use crate::ground_control::core::TcpLink;
use crate::ground_control::monitoring::GcsMonitor;
use crate::ground_control::types::{InterlockState, ScheduledCommand};
use crate::satellite::types::{CommandKind, CommandMsg, LinkMsg};

fn to_tokio_instant(t: chrono::DateTime<chrono::Utc>) -> Instant {
    let now = Utc::now();
    let dur = if t > now {
        (t - now).to_std().unwrap_or(Duration::from_millis(0))
    } else {
        Duration::from_millis(0)
    };
    Instant::now() + dur
}

async fn sleep_precise(target: Instant) {
    let now = Instant::now();
    if target > now + Duration::from_millis(2) {
        sleep_until(target - Duration::from_millis(2)).await;
    }
    while Instant::now() < target {
        tokio::task::yield_now().await;
    }
}

pub async fn run_commands_22(mut link: TcpLink) -> Result<(), String> {
    let mut queue: Vec<ScheduledCommand> = vec![
        ScheduledCommand {
            id: 0,
            kind: CommandKind::SetModeSafe,
            dispatch_at: Utc::now() + ChronoDur::seconds(1),
            deadline_ms: 2,
        },
        ScheduledCommand {
            id: 1,
            kind: CommandKind::AntennaAlign,
            dispatch_at: Utc::now() + ChronoDur::seconds(2),
            deadline_ms: 2,
        },
        ScheduledCommand {
            id: 2,
            kind: CommandKind::ResetSubsystem,
            dispatch_at: Utc::now() + ChronoDur::seconds(5),
            deadline_ms: 2,
        },
        ScheduledCommand {
            id: 3,
            kind: CommandKind::ResetSubsystem,
            dispatch_at: Utc::now() + ChronoDur::seconds(15),
            deadline_ms: 2,
        },
    ];

    let mut monitor = GcsMonitor::new();
    let mut interlock = InterlockState::default();

    let mut fault_received_at: Option<Instant> = None;
    let mut dispatched_at: HashMap<u64, Instant> = HashMap::new();

    let mut last_summary = Instant::now();
    let mut dispatch_count: u64 = 0;
    let mut reject_count: u64 = 0;

    loop {
        queue.sort_by_key(|c| c.dispatch_at);
        let next = queue.first().cloned();

        tokio::select! {
            biased;

            _ = async {
                if let Some(cmd) = &next {
                    sleep_precise(to_tokio_instant(cmd.dispatch_at)).await;
                } else {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            } => {}

            msg = link.rx.recv() => {
                let Some(msg) = msg else {
                    warn!("(GCS) peer disconnected");
                    break;
                };

                match msg {
                    LinkMsg::Fault(f) => {
                        fault_received_at = Some(Instant::now());
                        interlock.fault_active = true;

                        // FIX: clone dulu supaya gak move error
                        interlock.last_fault = Some(f.code.clone());

                        warn!("(GCS) FAULT received: {:?} detail={}", f.code, f.detail);
                    }

                    LinkMsg::Telemetry(t) => {
                        info!("(GCS) TELEMETRY seq={} sensor={:?} prio={}", t.seq, t.sensor, t.priority);
                        let (req, loc) = monitor.on_telemetry(&t);

                        if let Some(req_msg) = req {
                            let _ = link.tx.send(req_msg).await;
                        }
                        if let Some(loc_msg) = loc {
                            let _ = link.tx.send(loc_msg).await;
                        }
                    }

                    LinkMsg::CommandAck { id, .. } => {
                        if let Some(t0) = dispatched_at.remove(&id) {
                            let rtt_ms = t0.elapsed().as_millis();
                            info!("(GCS) COMMAND RESPONSE latency_ms={} for_id={}", rtt_ms, id);
                        }
                    }

                    _ => {}
                }

                if last_summary.elapsed().as_secs() >= 5 {
                    info!(
                        "(GCS) SYSTEM summary dispatch_count={} reject_count={} fault_active={}",
                        dispatch_count, reject_count, interlock.fault_active
                    );
                    last_summary = Instant::now();
                }

                continue;
            }
        }

        queue.sort_by_key(|c| c.dispatch_at);
        if queue.is_empty() {
            continue;
        }

        let cmd = queue.remove(0);
        let now = Utc::now();

        let unsafe_cmd = matches!(cmd.kind, CommandKind::ResetSubsystem | CommandKind::AntennaAlign);

        if interlock.fault_active && unsafe_cmd {
            let lat_ms = fault_received_at.map(|t| t.elapsed().as_millis()).unwrap_or(0);
            warn!("(GCS) INTERLOCK latency={}ms (fault->block)", lat_ms);
            if lat_ms > 100 {
                warn!("(GCS) CRITICAL GROUND ALERT: fault response time >100ms");
            }
            warn!("(GCS) COMMAND REJECTED id={} kind={:?} reason=INTERLOCK", cmd.id, cmd.kind);
            reject_count += 1;
            continue;
        }

        let lateness_ms = (now - cmd.dispatch_at).num_milliseconds();
        if cmd.deadline_ms <= 2 && lateness_ms > 2 {
            warn!("(GCS) URGENT DISPATCH LATE id={} lateness_ms={} (>2ms)", cmd.id, lateness_ms);
        } else if cmd.deadline_ms <= 2 {
            info!("(GCS) URGENT DISPATCH ON-TIME id={} lateness_ms={}", cmd.id, lateness_ms);
        }

        let msg = CommandMsg {
            id: cmd.id,
            kind: cmd.kind.clone(),
            issued_at: now,
            deadline_ms: cmd.deadline_ms,
        };

        let _ = link.tx.send(LinkMsg::Command(msg)).await;
        dispatched_at.insert(cmd.id, Instant::now());
        dispatch_count += 1;
        info!("(GCS) COMMAND DISPATCHED id={} kind={:?}", cmd.id, cmd.kind);
    }

    Err("GCS commands loop ended".into())
}
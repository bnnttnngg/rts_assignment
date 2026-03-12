use chrono::{DateTime, Duration as ChronoDur, Utc};
use tokio::time::{sleep_until, Instant};
use tracing::{info, warn};

use crate::ground_control::core::TcpLink;
use crate::ground_control::monitoring::GcsMonitor;
use crate::ground_control::types::{InterlockState, ScheduledCommand};
use crate::satellite::types::{CommandKind, CommandMsg, LinkMsg};

fn chrono_to_tokio_instant(t: &DateTime<Utc>) -> Instant {
    let now = Utc::now();
    let dur = if *t > now {
        (*t - now).to_std().unwrap_or(std::time::Duration::from_millis(0))
    } else {
        std::time::Duration::from_millis(0)
    };
    Instant::now() + dur
}

pub async fn run_commands_22(mut link: TcpLink) -> Result<(), String> {
    // schedule demo
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
    ];

    let mut monitor = GcsMonitor::new();
    let mut interlock = InterlockState::default();

    // time when fault first received (for latency measurement)
    let mut fault_rx_at: Option<Instant> = None;

    loop {
        queue.sort_by_key(|c| c.dispatch_at);
        let next_due = queue.first().map(|c| chrono_to_tokio_instant(&c.dispatch_at));

        tokio::select! {
            msg = link.rx.recv() => {
                let Some(msg) = msg else {
                    warn!("(GCS) peer disconnected");
                    break;
                };

                match msg {
                    LinkMsg::Fault(f) => {
                        // FIX: clone code so we can store & also log
                        let code = f.code.clone();

                        if !interlock.fault_active {
                            fault_rx_at = Some(Instant::now());
                        }
                        interlock.fault_active = true;
                        interlock.last_fault = Some(code.clone());

                        warn!("(GCS) FAULT received: {:?} detail={}", code, f.detail);
                    }

                    LinkMsg::Telemetry(t) => {
                        info!("(GCS) TELEMETRY seq={} sensor={:?} prio={}", t.seq, t.sensor, t.priority);

                        let (req, loc_fault) = monitor.on_telemetry(&t);
                        if let Some(req_msg) = req {
                            let _ = link.tx.send(req_msg).await;
                        }
                        if let Some(loc) = loc_fault {
                            let _ = link.tx.send(loc).await;
                        }
                    }

                    _ => {}
                }
            }

            _ = async {
                if let Some(t) = next_due {
                    sleep_until(t).await;
                }
            }, if next_due.is_some() => {}
        }

        // dispatch block
        queue.sort_by_key(|c| c.dispatch_at);
        if queue.is_empty() {
            continue;
        }

        let cmd = queue.first().cloned().unwrap();
        let now = Utc::now();

        if now < cmd.dispatch_at {
            continue;
        }

        let unsafe_cmd = matches!(cmd.kind, CommandKind::ResetSubsystem | CommandKind::AntennaAlign);

        // interlock blocks unsafe command
        if interlock.fault_active && unsafe_cmd {
            let since_fault_ms = fault_rx_at.map(|t| t.elapsed().as_millis()).unwrap_or(0);
            warn!(
                "(GCS) COMMAND REJECTED id={} kind={:?} reason=INTERLOCK since_fault_ms={}",
                cmd.id, cmd.kind, since_fault_ms
            );

            if since_fault_ms > 100 {
                warn!("(GCS) CRITICAL GROUND ALERT: fault response time >100ms");
            }

            queue.remove(0);
            continue;
        }

        // urgent deadline check: lateness = now - dispatch_at
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

        link.tx.send(LinkMsg::Command(msg)).await.map_err(|_| "send failed".to_string())?;
        info!("(GCS) COMMAND DISPATCHED id={} kind={:?}", cmd.id, cmd.kind);

        queue.remove(0);
    }

    Err("GCS loop ended".into())
}
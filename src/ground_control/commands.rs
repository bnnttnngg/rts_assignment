use chrono::{Duration as ChronoDur, Utc};
use std::time::Instant;

use tokio::time::{sleep, Duration};
use tracing::{info, warn};

use crate::ground_control::core::TcpLink;
use crate::ground_control::monitoring::GcsMonitor;
use crate::ground_control::types::{InterlockState, ScheduledCommand};
use crate::satellite::types::{CommandKind, CommandMsg, LinkMsg};

pub async fn run_commands_22(mut link: TcpLink) -> Result<(), String> {
    // schedule commands
    let mut queue: Vec<ScheduledCommand> = vec![
        // SAFE cmd: selalu boleh jalan, buat bukti SAT receive command
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

    let mut interlock = InterlockState::default();
    let mut monitor = GcsMonitor::new();

    // measure interlock latency (fault detect -> block)
    let mut fault_detected_at: Option<Instant> = None;

    // urgent dispatch timing (simple)
    let mut tick = Instant::now();

    loop {
        // 1) receive inbound from SAT
        while let Ok(msg) = link.rx.try_recv() {
            match msg {
                LinkMsg::Fault(f) => {
                    if !interlock.fault_active {
                        fault_detected_at = Some(Instant::now());
                    }
                    interlock.fault_active = true;
                    interlock.last_fault = Some(f.code.clone()); // ✅ fix f.code merah
                    warn!("(GCS) FAULT received: {:?} detail={}", f.code, f.detail);
                }

                LinkMsg::Telemetry(t) => {
                    info!("(GCS) TELEMETRY seq={} sensor={:?} prio={}", t.seq, t.sensor, t.priority);

                    let (req, loc_fault) = monitor.on_telemetry(&t);

                    if let Some(req_msg) = req {
                        let _ = link.tx.send(req_msg).await;
                    }

                    if let Some(loc) = loc_fault {
                        warn!("(GCS) LOSS-OF-CONTACT triggered -> fault");
                        let _ = link.tx.send(loc).await; // optional
                    }
                }

                _ => {}
            }
        }

        // 2) dispatch commands
        queue.sort_by_key(|c| c.dispatch_at);
        let now = Utc::now();

        if let Some(cmd) = queue.first().cloned() {
            if cmd.dispatch_at <= now {
                let unsafe_cmd = matches!(cmd.kind, CommandKind::ResetSubsystem | CommandKind::AntennaAlign);

                // interlock block
                if interlock.fault_active && unsafe_cmd {
                    if let Some(t0) = fault_detected_at.take() {
                        let lat_ms = t0.elapsed().as_millis();
                        warn!("(GCS) INTERLOCK latency={}ms (fault->block)", lat_ms);
                        if lat_ms > 100 {
                            warn!("(GCS) CRITICAL GROUND ALERT: fault response time >100ms");
                        }
                    }

                    warn!(
                        "(GCS) COMMAND REJECTED id={} kind={:?} reason=INTERLOCK fault_active=true",
                        cmd.id, cmd.kind
                    );
                    queue.remove(0);
                    continue;
                }

                // urgent dispatch deadline <=2ms (demo)
                let late_ms = tick.elapsed().as_millis();
                if cmd.deadline_ms <= 2 && late_ms > 2 {
                    warn!("(GCS) URGENT DISPATCH LATE id={} late_ms={}", cmd.id, late_ms);
                }
                tick = Instant::now();

                let msg = CommandMsg {
                    id: cmd.id,
                    kind: cmd.kind.clone(),
                    issued_at: now,
                    deadline_ms: cmd.deadline_ms,
                };

                link.tx
                    .send(LinkMsg::Command(msg))
                    .await
                    .map_err(|_| "send failed".to_string())?;

                info!("(GCS) COMMAND DISPATCHED id={} kind={:?}", cmd.id, cmd.kind);
                queue.remove(0);
            }
        }

        sleep(Duration::from_millis(1)).await;
    }
}
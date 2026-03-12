use chrono::{Duration as ChronoDur, Utc};
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

use crate::ground_control::core::TcpLink;
use crate::ground_control::types::{InterlockState, ScheduledCommand};
use crate::satellite::types::{CommandKind, CommandMsg, LinkMsg};

pub async fn run_commands_22(mut link: TcpLink) -> Result<(), String> {
    // Example schedule (kau boleh ubah ikut demo)
    let mut queue: Vec<ScheduledCommand> = vec![
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

    loop {
        // =========================
        // (1) RECEIVE inbound
        // =========================
        while let Ok(msg) = link.rx.try_recv() {
            match msg {
                LinkMsg::Fault(f) => {
                    // ❗tak guna f.code (avoid error field merah)
                    interlock.fault_active = true;
                    warn!("(GCS) FAULT received: {:?}", f);
                }
                LinkMsg::Telemetry(t) => {
                    info!("(GCS) TELEMETRY received: {:?}", t);
                }
                LinkMsg::Ack { msg, .. } => {
                    info!("(GCS) ACK: {}", msg);
                }
                LinkMsg::Hello { who, .. } => {
                    info!("(GCS) HELLO from {}", who);
                }
                _ => {}
            }
        }

        // =========================
        // (2) DISPATCH command due
        // =========================
        queue.sort_by_key(|c| c.dispatch_at);

        if let Some(cmd) = queue.first().cloned() {
            let now = Utc::now();
            if cmd.dispatch_at <= now {
                // Safety interlock: bila fault aktif, block unsafe cmds
                let unsafe_cmd = matches!(cmd.kind, CommandKind::ResetSubsystem | CommandKind::AntennaAlign);
                if interlock.fault_active && unsafe_cmd {
                    warn!(
                        "(GCS) COMMAND REJECTED id={} kind={:?} reason=INTERLOCK fault_active={}",
                        cmd.id, cmd.kind, interlock.fault_active
                    );
                    queue.remove(0);
                    continue;
                }

                // urgent deadline check <=2ms (simple)
                let late_ms = (now - cmd.dispatch_at).num_milliseconds();
                if cmd.deadline_ms <= 2 && late_ms > 2 {
                    warn!("(GCS) URGENT DISPATCH LATE id={} late_ms={}", cmd.id, late_ms);
                }

                let out = CommandMsg {
                    id: cmd.id,
                    kind: cmd.kind.clone(),
                    issued_at: now,
                    deadline_ms: cmd.deadline_ms,
                };

                link.tx
                    .send(LinkMsg::Command(out))
                    .await
                    .map_err(|_| "GCS send command failed".to_string())?;

                info!("(GCS) COMMAND DISPATCHED id={} kind={:?}", cmd.id, cmd.kind);
                queue.remove(0);
            }
        }

        sleep(Duration::from_millis(1)).await;
    }
}
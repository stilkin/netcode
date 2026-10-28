//! Hub-side spawn + watchdog: a worker that exits immediately must produce a
//! pending session that closes, and its id must never be reused.

use hub::backplane_service::{Service, ServiceHandle, Timeouts};
use hub::config::BackplaneConfig;
use hub::registry::SessionSpec;
use hub::spawn::SpawnRequest;
use std::time::Duration;

async fn start_service() -> (ServiceHandle, std::net::SocketAddr) {
    let cfg = BackplaneConfig {
        bind: "127.0.0.1:0".into(),
        token: String::new(),
        max_sessions: 64,
    };
    let service = Service::bind_with_timeouts(
        &cfg,
        Timeouts {
            registration: Duration::from_millis(300),
            liveness: Duration::from_millis(800),
            pending: Duration::from_millis(400),
        },
    )
    .await
    .expect("bind");
    let addr = service.local_addr();
    (service.start(), addr)
}

fn request() -> SpawnRequest {
    SpawnRequest {
        program: std::env!("CARGO_BIN_EXE_sample-worker").into(),
        spec: SessionSpec {
            game_type: "echo-arena".into(),
            display_name: "Echo Arena".into(),
            max_players: 8,
            payload_schema_version: 1,
        },
        session_config_json: "{}".into(),
        extra_args: vec!["--exit-immediately".into()],
    }
}

#[tokio::test]
async fn exiting_worker_closes_its_session_and_ids_are_not_reused() {
    let (service, _addr) = start_service().await;
    let first = service.spawn_session(request()).await.expect("spawn");

    // The pending session closes once the child exits (watchdog), well before
    // the pending timeout would also remove it.
    let mut gone = false;
    for _ in 0..40 {
        if service.sessions().await.is_empty() {
            gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(gone, "watchdog must close the session of a dead worker");

    // Ids are monotonic: the closed id is never handed out again.
    let second = service
        .create_pending(SessionSpec {
            game_type: "other".into(),
            display_name: "Other".into(),
            max_players: 8,
            payload_schema_version: 1,
        })
        .await
        .expect("pending");
    assert!(second > first);

    service.shutdown();
    service.drain().await;
}

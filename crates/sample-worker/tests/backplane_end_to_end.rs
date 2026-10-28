//! End to end: in-process hub + spawned `sample-worker` child process.
//!
//! Covers the full loop the roadmap promises: spawn → pending → active →
//! attach → ordered client inputs → per-client echoes → broadcasts →
//! detach → worker loss closes the session → ids never reused.

use std::time::Duration;

use hub::backplane_service::{Service, ServiceHandle, Timeouts};
use hub::config::BackplaneConfig;
use hub::registry::{SessionSpec, SessionState};
use hub::spawn::SpawnRequest;
use protocol::Header;

const TICK_HZ_JSON: &str = r#"{"tick_hz":60}"#;

async fn start_service() -> (ServiceHandle, std::net::SocketAddr) {
    let cfg = BackplaneConfig {
        bind: "127.0.0.1:0".into(),
        token: String::new(),
        max_sessions: 64,
    };
    let service = Service::bind_with_timeouts(
        &cfg,
        Timeouts {
            registration: Duration::from_secs(2),
            liveness: Duration::from_secs(5),
            pending: Duration::from_secs(10),
        },
    )
    .await
    .expect("bind");
    let addr = service.local_addr();
    (service.start(), addr)
}

fn spawn_request(extra_args: &[&str]) -> SpawnRequest {
    SpawnRequest {
        program: std::env!("CARGO_BIN_EXE_sample-worker").into(),
        spec: SessionSpec {
            game_type: "echo-arena".into(),
            display_name: "Echo Arena".into(),
            max_players: 8,
            payload_schema_version: 1,
        },
        session_config_json: TICK_HZ_JSON.into(),
        extra_args: extra_args.iter().map(|s| s.to_string()).collect(),
    }
}

/// Waits until the async `check` succeeds.
async fn poll_until(mut check: impl AsyncFnMut() -> bool, secs: u64, what: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    loop {
        if check().await {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("timed out waiting for: {what}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn packet(session_id: u32, client_id: u32, payload: &[u8]) -> Vec<u8> {
    Header::new(0, session_id, client_id, 1, payload.len() as u32)
        .encode_packet(payload)
        .expect("packet")
}

#[tokio::test]
async fn full_loop_with_spawned_worker() {
    let (service, _addr) = start_service().await;

    // Spawn: the session must go pending → active once the worker claims it.
    let session_id = service
        .spawn_session(spawn_request(&[]))
        .await
        .expect("spawn");
    poll_until(
        || async {
            service
                .sessions()
                .await
                .iter()
                .any(|s| s.id == session_id && s.state == SessionState::Active)
        },
        5,
        "worker to claim its session",
    )
    .await;

    // Attach two players; each gets a greeting on the events channel. The
    // worker also broadcasts ticks right away, so read through STATE frames
    // until the greeting event arrives.
    let mut c1 = service.attach(session_id, "p1").await.expect("attach c1");
    let mut c2 = service.attach(session_id, "p2").await.expect("attach c2");
    for (client, id) in [(&mut c1, 1u32), (&mut c2, 2u32)] {
        loop {
            let got = client.recv().await.expect("client queue open");
            let (header, payload) = Header::split_packet(&got).expect("packet");
            match header.channel {
                hub_sdk::CHANNEL_EVENTS => {
                    assert_eq!(header.client_id, id, "greeting is unicast");
                    assert_eq!(payload, format!("welcome {id}").as_bytes());
                    break;
                }
                hub_sdk::CHANNEL_STATE => {
                    if header.client_id == 0 {
                        continue; // a broadcast interleaving with the echoes
                    }
                    // Ticks broadcast with client_id 0; echoes name a client.
                    assert!(header.client_id == 0 || header.client_id == id);
                }
                other => panic!("unknown channel {other}"),
            }
        }
    }

    // Three ordered inputs from c1 must be echoed back in order. Broadcasts
    // interleave, so route by recipient while collecting.
    for payload in ["i1", "i2", "i3"] {
        c1.feed_packet(packet(session_id, c1.id, payload.as_bytes()));
    }
    let mut echoes: Vec<String> = Vec::new();
    let mut c2_broadcasts: Vec<u64> = Vec::new();
    while echoes.len() < 3 {
        let got = tokio::time::timeout(Duration::from_secs(5), c1.recv())
            .await
            .expect("c1 output in time")
            .expect("c1 queue open");
        let (header, payload) = Header::split_packet(&got).expect("packet");
        match header.channel {
            hub_sdk::CHANNEL_STATE => {
                if header.client_id == 0 {
                    continue; // a broadcast interleaving with the echoes
                }
                assert_eq!(
                    header.client_id, c1.id,
                    "echoes are unicast to their sender"
                );
                echoes.push(String::from_utf8(payload.to_vec()).expect("utf8"));
            }
            hub_sdk::CHANNEL_EVENTS => {}
            other => panic!("unknown channel {other}"),
        }
    }
    assert_eq!(echoes, vec!["i1", "i2", "i3"], "inputs echoed in order");

    // c2 has no inputs, so everything it receives on the state channel is a
    // broadcast — strictly increasing tick counters, one per tick.
    while c2_broadcasts.len() < 3 {
        let got = tokio::time::timeout(Duration::from_secs(5), c2.recv())
            .await
            .expect("c2 output in time")
            .expect("c2 queue open");
        let (header, payload) = Header::split_packet(&got).expect("packet");
        match header.channel {
            hub_sdk::CHANNEL_STATE => {
                assert_eq!(header.client_id, 0, "c2 only receives broadcasts");
                let tick = u64::from_le_bytes(payload.try_into().expect("tick counter"));
                if let Some(last) = c2_broadcasts.last() {
                    assert!(
                        tick > *last,
                        "tick counters strictly increase (no duplicates)"
                    );
                }
                c2_broadcasts.push(tick);
            }
            hub_sdk::CHANNEL_EVENTS => {}
            other => panic!("unknown channel {other}"),
        }
    }

    // Detaching c2 (dropping the handle) decrements the player count.
    drop(c2);
    poll_until(
        || async {
            service
                .sessions()
                .await
                .iter()
                .find(|s| s.id == session_id)
                .is_some_and(|s| s.player_count == 1)
        },
        5,
        "player count to drop after detach",
    )
    .await;

    // Worker loss (a clean child exit here) closes the active session.
    let doomed = service
        .spawn_session(spawn_request(&["--exit-after", "1"]))
        .await
        .expect("spawn short-lived worker");
    let _ = doomed;
    let before = service
        .sessions()
        .await
        .iter()
        .map(|s| s.id)
        .collect::<Vec<_>>();
    poll_until(
        || async { !service.sessions().await.iter().any(|s| s.id == doomed) },
        5,
        "short-lived worker's session to close",
    )
    .await;

    // Ids are monotonic: closed ids are never handed out again.
    let fresh = service
        .create_pending(SessionSpec {
            game_type: "probe".into(),
            display_name: "Probe".into(),
            max_players: 1,
            payload_schema_version: 1,
        })
        .await
        .expect("pending");
    assert!(
        fresh > session_id && fresh > doomed,
        "{fresh} must exceed {before:?}"
    );

    service.shutdown();
    service.drain().await;
}

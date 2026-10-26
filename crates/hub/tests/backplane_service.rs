//! Scripted fake workers over real 127.0.0.1 sockets, driving the service
//! through its public seam.

use std::time::Duration;

use backplane::{Frame, conn};
use hub::backplane_service::{Service, ServiceHandle, Timeouts};
use hub::config::BackplaneConfig;
use hub::registry::{SessionSpec, SessionState};
use protocol::Header;

const FAST: Timeouts = Timeouts {
    registration: Duration::from_millis(300),
    liveness: Duration::from_millis(800),
    pending: Duration::from_millis(400),
};

async fn start_service(token: &str) -> (ServiceHandle, std::net::SocketAddr) {
    let cfg = BackplaneConfig {
        bind: "127.0.0.1:0".into(),
        token: token.into(),
    };
    let service = Service::bind_with_timeouts(&cfg, FAST).await.expect("bind");
    let addr = service.local_addr();
    (service.start(), addr)
}

fn spec(max_players: u16) -> SessionSpec {
    SessionSpec {
        game_type: "echo-arena".into(),
        display_name: "Echo Arena".into(),
        max_players,
        payload_schema_version: 1,
    }
}

/// Connects, authenticates, registers, returns the link + assigned id.
async fn fake_worker(
    addr: std::net::SocketAddr,
    token: &str,
    claim: u32,
    spec: &SessionSpec,
) -> (
    tokio::net::tcp::OwnedReadHalf,
    tokio::net::tcp::OwnedWriteHalf,
    u32,
) {
    let stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let (mut rd, mut wr) = stream.into_split();
    conn::write_frame(
        &mut wr,
        &Frame::Hello {
            token: token.into(),
        },
    )
    .await
    .expect("Hello");
    conn::write_frame(
        &mut wr,
        &Frame::Register {
            claimed_id: claim,
            game_type: spec.game_type.clone(),
            display_name: spec.display_name.clone(),
            max_players: spec.max_players,
            payload_schema_version: spec.payload_schema_version,
        },
    )
    .await
    .expect("Register");
    let registered = tokio::time::timeout(Duration::from_secs(2), conn::read_frame(&mut rd))
        .await
        .expect("Registered in time")
        .expect("Registered parses");
    let session_id = match registered {
        Frame::Registered { session_id } => session_id,
        other => panic!("expected Registered, got {other:?}"),
    };
    (rd, wr, session_id)
}

/// The hub must close the connection: any read error (typically EOF).
async fn expect_closed(rd: &mut tokio::net::tcp::OwnedReadHalf) {
    let read = tokio::time::timeout(Duration::from_secs(2), conn::read_frame(rd)).await;
    match read {
        Err(_) => panic!("hub should close promptly"),
        Ok(Err(_)) => {} // closed — what we want
        Ok(Ok(frame)) => panic!("expected closure, got {frame:?}"),
    }
}

#[tokio::test]
async fn wrong_token_is_rejected() {
    let (service, addr) = start_service("s3cret").await;
    let stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let (mut rd, mut wr) = stream.into_split();
    conn::write_frame(
        &mut wr,
        &Frame::Hello {
            token: "nope".into(),
        },
    )
    .await
    .expect("Hello");
    conn::write_frame(
        &mut wr,
        &Frame::Register {
            claimed_id: 0,
            game_type: "g".into(),
            display_name: "n".into(),
            max_players: 8,
            payload_schema_version: 1,
        },
    )
    .await
    .expect("Register");
    expect_closed(&mut rd).await;
    assert!(
        service.sessions().await.is_empty(),
        "rejected workers create no sessions"
    );
    service.shutdown();
    service.drain().await;
}

#[tokio::test]
async fn silent_worker_is_reaped() {
    let (service, addr) = start_service("").await;
    let (mut rd, _wr, _session_id) = fake_worker(addr, "", 0, &spec(8)).await;
    assert_eq!(service.sessions().await.len(), 1);
    // No heartbeats: the hub must reap the connection past liveness.
    expect_closed(&mut rd).await;
    assert!(service.sessions().await.is_empty());
    service.shutdown();
    service.drain().await;
}

#[tokio::test]
async fn bad_claim_closes_connection_but_keeps_pending() {
    let (service, addr) = start_service("").await;
    let pending = service.create_pending(spec(8)).await;
    let stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let (mut rd, mut wr) = stream.into_split();
    conn::write_frame(
        &mut wr,
        &Frame::Hello {
            token: String::new(),
        },
    )
    .await
    .expect("Hello");
    conn::write_frame(
        &mut wr,
        &Frame::Register {
            claimed_id: 999,
            game_type: "echo-arena".into(),
            display_name: "Echo Arena".into(),
            max_players: 8,
            payload_schema_version: 1,
        },
    )
    .await
    .expect("Register");
    expect_closed(&mut rd).await;
    // The real pending session is untouched (until its own timeout).
    let sessions = service.sessions().await;
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, pending);
    assert_eq!(sessions[0].state, SessionState::Pending);
    service.shutdown();
    service.drain().await;
}

#[tokio::test]
async fn fanout_routing_and_stale_unicast() {
    let (service, addr) = start_service("").await;
    let (mut rd, mut wr, session_id) = fake_worker(addr, "", 0, &spec(8)).await;
    let mut c1 = service.attach(session_id, "p1").await.expect("attach c1");
    let mut c2 = service.attach(session_id, "p2").await.expect("attach c2");

    // ToClient routes by the packet header's client id.
    conn::write_frame(
        &mut wr,
        &Frame::ToClient {
            packet: Header::new(0, session_id, c1.id, 1, 2)
                .encode_packet(b"c1")
                .expect("packet"),
        },
    )
    .await
    .expect("send");
    let got = c1.recv().await.expect("c1 receives");
    let (_, payload) = Header::split_packet(&got).expect("packet");
    assert_eq!(payload, b"c1");

    // Broadcast reaches every attached client exactly once.
    conn::write_frame(
        &mut wr,
        &Frame::Broadcast {
            packet: Header::new(0, session_id, 0, 1, 5)
                .encode_packet(b"state")
                .expect("packet"),
        },
    )
    .await
    .expect("send");
    for client in [&mut c1, &mut c2] {
        let got = client.recv().await.expect("broadcast copy");
        let (_, payload) = Header::split_packet(&got).expect("packet");
        assert_eq!(payload, b"state");
    }

    // ToMany respects the explicit list.
    conn::write_frame(
        &mut wr,
        &Frame::ToMany {
            client_ids: vec![c2.id],
            packet: Header::new(0, session_id, c2.id, 1, 2)
                .encode_packet(b"c2")
                .expect("packet"),
        },
    )
    .await
    .expect("send");
    let got = c2.recv().await.expect("c2 receives");
    let (_, payload) = Header::split_packet(&got).expect("packet");
    assert_eq!(payload, b"c2");

    // Stale unicast to an unknown client is dropped silently; the link
    // must stay alive — the next broadcast still arrives.
    conn::write_frame(
        &mut wr,
        &Frame::ToClient {
            packet: Header::new(0, session_id, 999, 1, 5)
                .encode_packet(b"stale")
                .expect("packet"),
        },
    )
    .await
    .expect("send stale");
    conn::write_frame(
        &mut wr,
        &Frame::Broadcast {
            packet: Header::new(0, session_id, 0, 1, 5)
                .encode_packet(b"alive")
                .expect("packet"),
        },
    )
    .await
    .expect("send");
    let got = c1.recv().await.expect("still connected");
    let (_, payload) = Header::split_packet(&got).expect("packet");
    assert_eq!(payload, b"alive");

    // Drain the ClientAttached notifications queued by the two attaches.
    for _ in 0..2 {
        let frame = conn::read_frame(&mut rd)
            .await
            .expect("attach notification");
        assert!(matches!(frame, Frame::ClientAttached { .. }), "{frame:?}");
    }

    // Client→worker packets are forwarded in order.
    for payload in ["i1", "i2", "i3"] {
        c1.feed_packet(
            Header::new(0, session_id, c1.id, 1, payload.len() as u32)
                .encode_packet(payload.as_bytes())
                .expect("packet"),
        );
    }
    for expected in ["i1", "i2", "i3"] {
        let frame = tokio::time::timeout(Duration::from_secs(2), conn::read_frame(&mut rd))
            .await
            .expect("worker receives input")
            .expect("worker link open");
        match frame {
            Frame::FromClient { packet } => {
                let (_, got) = Header::split_packet(&packet).expect("packet");
                assert_eq!(got, expected.as_bytes(), "inputs arrive in order");
            }
            other => panic!("expected FromClient, got {other:?}"),
        }
    }

    service.shutdown();
    service.drain().await;
}

#[tokio::test]
async fn disconnect_closes_session_and_detaches_clients() {
    let (service, addr) = start_service("").await;
    let (rd, wr, session_id) = fake_worker(addr, "", 0, &spec(8)).await;
    let mut client = service.attach(session_id, "p1").await.expect("attach");
    drop((rd, wr)); // worker vanishes

    // Poll until the hub notices.
    let mut gone = false;
    for _ in 0..40 {
        if service.sessions().await.is_empty() {
            gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(gone, "session must disappear after worker loss");
    assert!(client.recv().await.is_none(), "client queue is closed");
    service.shutdown();
    service.drain().await;
}

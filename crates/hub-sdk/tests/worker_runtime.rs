//! The SDK worker runtime against a scripted stub hub, over real TCP.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use hub_sdk::{CHANNEL_STATE, ClientInput, GameInfo, GameSession, SessionCtx, WorkerConfig, run};
use protocol::Header;

type Inputs = Arc<Mutex<Vec<(u32, Vec<u8>)>>>;

#[derive(Default, Clone)]
struct Recording {
    attaches: Arc<Mutex<Vec<u32>>>,
    detaches: Arc<Mutex<Vec<u32>>>,
    inputs: Inputs,
    ticks: Arc<Mutex<Vec<u32>>>,
}

impl Recording {
    fn game(&self) -> RecordingGame {
        RecordingGame(self.clone())
    }
}

struct RecordingGame(Recording);

impl GameSession for RecordingGame {
    fn on_attach(&mut self, _ctx: &SessionCtx, client_id: u32, _player_name: &str) {
        self.0.attaches.lock().unwrap().push(client_id);
    }

    fn on_detach(&mut self, _ctx: &SessionCtx, client_id: u32) {
        self.0.detaches.lock().unwrap().push(client_id);
    }

    fn tick(&mut self, ctx: &SessionCtx, _dt: f32, inputs: &[ClientInput]) {
        for input in inputs {
            self.0
                .inputs
                .lock()
                .unwrap()
                .push((input.client_id, input.payload.clone()));
            ctx.to_client(CHANNEL_STATE, input.client_id, &input.payload);
        }
        self.0.ticks.lock().unwrap().push(ctx.session_id());
        ctx.broadcast(CHANNEL_STATE, b"tick");
    }
}

#[tokio::test]
async fn worker_runtime_full_loop() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");

    let recording = Recording::default();
    let worker = tokio::spawn(run(
        WorkerConfig {
            backplane_addr: addr.to_string(),
            token: "s3cret".into(),
            claim: None,
            info: GameInfo {
                game_type: "echo-arena".into(),
                display_name: "Echo Arena".into(),
                max_players: 8,
                payload_schema_version: 1,
            },
            session: hub_sdk::SessionConfig { tick_hz: 60 },
            heartbeat_interval: Duration::from_millis(50),
        },
        recording.game(),
    ));

    // --- stub hub: one connection, scripted ---
    let (stream, _) = listener.accept().await.expect("worker connects");
    let (mut rd, mut wr) = stream.into_split();

    // Hello carries the configured token.
    let hello = tokio::time::timeout(Duration::from_secs(5), backplane::conn::read_frame(&mut rd))
        .await
        .expect("Hello arrives")
        .expect("Hello parses");
    assert_eq!(
        hello,
        backplane::Frame::Hello {
            token: "s3cret".into()
        }
    );

    // Register asks for a fresh session.
    let register =
        tokio::time::timeout(Duration::from_secs(5), backplane::conn::read_frame(&mut rd))
            .await
            .expect("Register arrives")
            .expect("Register parses");
    assert!(matches!(
        register,
        backplane::Frame::Register { claimed_id: 0, .. }
    ));
    backplane::conn::write_frame(&mut wr, &backplane::Frame::Registered { session_id: 9 })
        .await
        .expect("write Registered");

    // Script: attach two clients, feed one input to client 1, detach client 2.
    for frame in [
        backplane::Frame::ClientAttached { client_id: 1 },
        backplane::Frame::ClientAttached { client_id: 2 },
        backplane::Frame::FromClient {
            packet: Header::new(CHANNEL_STATE, 9, 1, 1, 3)
                .encode_packet(b"abc")
                .expect("packet"),
        },
        backplane::Frame::ClientDetached { client_id: 2 },
    ] {
        backplane::conn::write_frame(&mut wr, &frame)
            .await
            .expect("script write");
    }

    // Read worker output until the input has been echoed — that proves a tick
    // ran with the buffered input.
    let mut broadcasts = 0;
    let mut seen = 0;
    loop {
        seen += 1;
        assert!(
            seen < 500,
            "input was never echoed; ticks not consuming inputs"
        );
        let frame =
            tokio::time::timeout(Duration::from_secs(5), backplane::conn::read_frame(&mut rd))
                .await
                .expect("worker output arrives")
                .expect("worker output parses");
        match frame {
            backplane::Frame::Heartbeat => {}
            backplane::Frame::Broadcast { .. } => broadcasts += 1,
            backplane::Frame::ToClient { packet } => {
                let (header, payload) = Header::split_packet(&packet).expect("client packet");
                assert_eq!(header.session_id, 9);
                assert_eq!(header.channel, CHANNEL_STATE);
                if payload == b"abc" {
                    assert_eq!(header.client_id, 1, "echo goes back to its sender");
                    break;
                }
            }
            other => panic!("unexpected frame {other:?}"),
        }
    }

    // Phase 2: the SDK must heartbeat on its own schedule; broadcasts from
    // subsequent ticks keep flowing while we wait.
    let mut heartbeats = 0;
    let mut seen = 0;
    loop {
        seen += 1;
        assert!(seen < 500, "no heartbeat observed");
        let frame =
            tokio::time::timeout(Duration::from_secs(5), backplane::conn::read_frame(&mut rd))
                .await
                .expect("worker output arrives")
                .expect("worker output parses");
        match frame {
            backplane::Frame::Heartbeat => heartbeats += 1,
            backplane::Frame::Broadcast { .. } => broadcasts += 1,
            _ => {}
        }
        if heartbeats >= 1 && broadcasts >= 1 {
            break;
        }
    }
    // Phase 2: the SDK must heartbeat on its own schedule.
    let mut heartbeats = 0;
    loop {
        let frame =
            tokio::time::timeout(Duration::from_secs(5), backplane::conn::read_frame(&mut rd))
                .await
                .expect("worker output arrives")
                .expect("worker output parses");
        if frame == backplane::Frame::Heartbeat {
            heartbeats += 1;
        }
        if heartbeats >= 1 {
            break;
        }
    }

    // Hub loss: the SDK returns cleanly.
    drop(wr);
    drop(rd);
    let outcome = tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .expect("worker finishes after hub loss")
        .expect("worker task");
    assert!(outcome.is_ok(), "hub loss is a clean stop: {outcome:?}");

    assert_eq!(*recording.attaches.lock().unwrap(), vec![1, 2]);
    assert_eq!(*recording.detaches.lock().unwrap(), vec![2]);
    assert_eq!(
        *recording.inputs.lock().unwrap(),
        vec![(1, b"abc".to_vec())]
    );
    assert!(!recording.ticks.lock().unwrap().is_empty());
}

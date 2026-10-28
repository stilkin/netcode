//! The real-game conformance run: the bot library drives the real echo
//! arena through a lobby-created session — create mode, real spawn, real
//! game, real everything.

use std::time::Duration;

use conformance::runner;
use conformance::scenario::Scenario;
use hub::backplane_service::{Service as Backplane, Timeouts};
use hub::client_edge::Service as Edge;
use hub::config::{BackplaneConfig, ClientEdgeConfig, GameConfig, LobbyConfig};
use hub::lobby::Service as Lobby;

/// (conformance: full loop against the real game) A create-mode scenario
/// spawns real echo-arena workers; bots join over WebTransport, deliver
/// their scripts, and every threshold holds.
#[tokio::test]
async fn bots_play_echo_arena_through_the_lobby() {
    let backplane = Backplane::bind_with_timeouts(
        &BackplaneConfig {
            bind: "127.0.0.1:0".into(),
            token: String::new(),
            max_sessions: 64,
        },
        Timeouts {
            registration: Duration::from_secs(2),
            liveness: Duration::from_secs(60),
            pending: Duration::from_secs(10),
        },
    )
    .await
    .expect("bind backplane")
    .start();

    let edge = Edge::bind(
        &ClientEdgeConfig {
            bind: "127.0.0.1:0".into(),
            token: String::new(),
        },
        backplane.clone(),
    )
    .await
    .expect("bind edge")
    .start();

    let games = [GameConfig {
        game_type: "echo-arena".into(),
        display_name: "Echo Arena".into(),
        max_players: 16,
        payload_schema_version: 1,
        worker: std::env!("CARGO_BIN_EXE_sample-worker").into(),
        session_config_json: r#"{"tick_hz":60}"#.into(),
    }];
    let lobby = Lobby::bind(
        &LobbyConfig {
            bind: "127.0.0.1:0".into(),
            token: String::new(),
        },
        &games,
        backplane.clone(),
        edge.cert_sha256(),
    )
    .await
    .expect("bind lobby")
    .start();

    let mut scenario = Scenario::parse(
        r#"
        [hub]
        lobby = "127.0.0.1:1"
        edge = "127.0.0.1:1"
        [sessions]
        mode = "create"
        game = "echo-arena"
        count = 2
        [bots]
        count = 6
        [script]
        inputs_per_bot = 20
        input_interval_ms = 4
        duration_ms = 15000
        [thresholds]
        join_success = 1.0
        max_loss_ratio = 0.3
        ordering_violations = 0
        "#,
    )
    .expect("scenario parses");
    scenario.hub.lobby = lobby.addr();
    scenario.hub.edge = edge.edge_addr();

    let report = runner::run(&scenario).await;
    assert_eq!(report.verdict, "pass", "checks: {:#?}", report.checks);
    assert_eq!(
        report.aggregate.joins_ok, 6,
        "all bots joined real sessions"
    );
    assert!(
        report.aggregate.echoes_received >= 6 * 10,
        "real echoes came back: {:?}",
        report.aggregate
    );
    // Every bot met the real game's greeting on the events stream.
    for bot in &report.bots {
        assert_eq!(bot.events.len(), 1, "one greeting each: {:?}", bot.events);
        assert!(
            bot.events[0].starts_with("welcome "),
            "greeting: {:?}",
            bot.events
        );
    }

    lobby.shutdown();
    lobby.drain().await;
    edge.shutdown();
    edge.drain().await;
    backplane.shutdown();
    backplane.clone().drain().await;
}

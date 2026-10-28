//! Load smoke profiles: declared N×M runs and stall isolation.

mod common;

use conformance::runner;
use conformance::scenario::Scenario;

/// (conformance: Load smoke profile) A declared multi-session profile runs
/// to completion meeting its thresholds.
#[tokio::test]
async fn smoke_profile_passes() {
    let rig = common::Rig::start().await;
    let sessions = vec![
        rig.scripted_session().await,
        rig.scripted_session().await,
        rig.scripted_session().await,
    ];

    // 18 bots across 3 sessions, 25 inputs each.
    let mut scenario = Scenario::parse(&format!(
        r#"
        [hub]
        lobby = "127.0.0.1:1"
        edge = "127.0.0.1:1"
        [sessions]
        mode = "join"
        ids = {:?}
        [bots]
        count = 18
        [script]
        inputs_per_bot = 25
        input_interval_ms = 3
        duration_ms = 20000
        [thresholds]
        join_success = 1.0
        max_loss_ratio = 0.25
        ordering_violations = 0
        "#,
        sessions
    ))
    .expect("scenario parses");
    scenario.hub.lobby = rig.lobby.addr();
    scenario.hub.edge = rig.edge.edge_addr();

    let report = runner::run(&scenario).await;
    assert_eq!(report.verdict, "pass", "checks: {:#?}", report.checks);
    assert_eq!(report.aggregate.joins_ok, 18);
    assert_eq!(report.aggregate.inputs_sent, 18 * 25);
    assert_eq!(report.aggregate.ordering_violations, 0);

    rig.stop().await;
}

/// (conformance: Load smoke profile) A deliberately stalled bot degrades
/// only itself; every other bot still meets its thresholds.
#[tokio::test]
async fn stalled_bot_is_isolated() {
    let rig = common::Rig::start().await;
    let session = rig.scripted_session().await;

    // 8 bots, one of which stalls right after joining.
    let mut scenario = Scenario::parse(&format!(
        r#"
        [hub]
        lobby = "127.0.0.1:1"
        edge = "127.0.0.1:1"
        [sessions]
        mode = "join"
        ids = [{session}]
        [bots]
        count = 8
        stall_after_join = 1
        [script]
        inputs_per_bot = 20
        input_interval_ms = 3
        duration_ms = 15000
        [thresholds]
        join_success = 1.0
        max_loss_ratio = 0.25
        ordering_violations = 0
        "#
    ))
    .expect("scenario parses");
    scenario.hub.lobby = rig.lobby.addr();
    scenario.hub.edge = rig.edge.edge_addr();

    let report = runner::run(&scenario).await;
    assert_eq!(report.verdict, "pass", "checks: {:#?}", report.checks);

    // The stalled probe is marked and excluded from the aggregates.
    let stalled = report
        .bots
        .iter()
        .find(|b| b.stalled)
        .expect("one stalled bot");
    assert!(stalled.echoes.is_empty(), "stalled bot reads nothing");
    assert_eq!(report.aggregate.joins_ok, 7, "only counted bots aggregate");

    // Everyone else round-tripped.
    for bot in report.bots.iter().filter(|b| !b.stalled) {
        assert!(
            bot.echoes.len() >= 15,
            "bot {}: {:?}",
            bot.bot,
            bot.echoes.len()
        );
    }

    rig.stop().await;
}

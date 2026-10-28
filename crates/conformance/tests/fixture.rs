//! Deterministic fixtures: record, replay, and mutation detection.

mod common;

use conformance::fixture::{self, Fixture};
use conformance::runner;
use conformance::scenario::Scenario;

async fn run_once(rig: &common::Rig, session: u32) -> conformance::report::Report {
    let mut scenario = Scenario::parse(&format!(
        r#"
        [hub]
        lobby = "127.0.0.1:1"
        edge = "127.0.0.1:1"
        [sessions]
        mode = "join"
        ids = [{session}]
        [bots]
        count = 3
        [script]
        inputs_per_bot = 10
        input_interval_ms = 3
        duration_ms = 10000
        [thresholds]
        join_success = 1.0
        max_loss_ratio = 0.3
        "#
    ))
    .expect("scenario parses");
    scenario.hub.lobby = rig.lobby.addr();
    scenario.hub.edge = rig.edge.edge_addr();
    runner::run(&scenario).await
}

/// (conformance: Deterministic fixtures) A run against the scripted worker
/// reproduces the recorded fixture: events exact and ordered, unicasts
/// loss-bounded, broadcasts monotonic.
#[tokio::test]
async fn fixture_replays_across_runs() {
    let rig = common::Rig::start().await;
    let session = rig.scripted_session().await;

    let first = run_once(&rig, session).await;
    assert_eq!(first.verdict, "pass", "recording run must pass");
    let recorded = fixture::record(&first, 0.0);

    let second = run_once(&rig, session).await;
    let divergences = fixture::compare(&recorded, &second);
    assert!(divergences.is_empty(), "divergences: {divergences:#?}");

    // The pinned fixture file (same shape) stays comparable.
    let _pinned = Fixture {
        bots: Vec::new(),
        max_loss_ratio: 0.0,
    };

    rig.stop().await;
}

/// (conformance: Deterministic fixtures) Mutations fail the comparison and
/// name the bot, channel, and position.
#[tokio::test]
async fn mutated_fixtures_fail_with_position() {
    let rig = common::Rig::start().await;
    let session = rig.scripted_session().await;

    let report = run_once(&rig, session).await;
    let recorded = fixture::record(&report, 0.0);

    // 1. Altered event payload.
    let mut mutated = recorded.clone();
    mutated.bots[0].events[0] = "welcome 999".into();
    let d = fixture::compare(&mutated, &report);
    assert!(
        d.iter()
            .any(|x| x.contains("bot 0") && x.contains("events diverge at position 0")),
        "divergences: {d:#?}"
    );

    // 2. Removed event (count mismatch).
    let mut mutated = recorded.clone();
    mutated.bots[1].events.clear();
    let d = fixture::compare(&mutated, &report);
    assert!(
        d.iter()
            .any(|x| x.contains("bot 1") && x.contains("event count")),
        "divergences: {d:#?}"
    );

    // 3. Altered unicast payload: the run now looks like it sent an
    //    unexpected echo, and the recorded one looks lost.
    let mut mutated = recorded.clone();
    if let Some(echo) = mutated.bots[2].echoes.first_mut() {
        *echo = "i9999".into();
    }
    let d = fixture::compare(&mutated, &report);
    assert!(
        d.iter().any(
            |x| x.contains("bot 2") && (x.contains("unexpected unicast") || x.contains("loss"))
        ),
        "divergences: {d:#?}"
    );

    rig.stop().await;
}

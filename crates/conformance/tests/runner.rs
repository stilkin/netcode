//! Scenario runner behavior: pass verdicts, threshold failures, reports.

mod common;

use conformance::runner;
use conformance::scenario::Scenario;

fn join_scenario(
    ids: Vec<u32>,
    bots: u32,
    inputs: u32,
    join_success: f64,
    max_loss: f64,
) -> Scenario {
    Scenario::parse(&format!(
        r#"
        [hub]
        lobby = "127.0.0.1:1"
        edge = "127.0.0.1:1"
        [sessions]
        mode = "join"
        ids = {ids:?}
        [bots]
        count = {bots}
        [script]
        inputs_per_bot = {inputs}
        input_interval_ms = 5
        duration_ms = 10000
        [thresholds]
        join_success = {join_success}
        max_loss_ratio = {max_loss}
        ordering_violations = 0
        "#
    ))
    .expect("scenario parses")
}

/// (conformance: Scenario runner) A healthy hub meets a reasonable
/// scenario; the report shows measured values.
#[tokio::test]
async fn passing_scenario_reports_measurements() {
    let rig = common::Rig::start().await;
    let sessions = vec![rig.scripted_session().await, rig.scripted_session().await];

    let mut scenario = join_scenario(sessions.clone(), 4, 15, 1.0, 0.25);
    scenario.hub.lobby = rig.lobby.addr();
    scenario.hub.edge = rig.edge.edge_addr();

    let report = runner::run(&scenario).await;
    assert_eq!(report.verdict, "pass", "checks: {:?}", report.checks);
    assert_eq!(report.aggregate.joins_ok, 4);
    assert_eq!(report.aggregate.joins_failed, 0);
    assert_eq!(report.aggregate.inputs_sent, 60);
    assert!(
        report.aggregate.echoes_received > 0,
        "echoes counted: {:?}",
        report.aggregate
    );
    assert!(report.checks.iter().any(|c| c.name == "max_loss_ratio"));
    assert!(
        report
            .bots
            .iter()
            .all(|b| b.events == vec!["welcome".to_owned()])
    );

    rig.stop().await;
}

/// (conformance: Scenario runner) Unmeetable thresholds fail with the
/// threshold named; unknown sessions fail cleanly.
#[tokio::test]
async fn failing_threshold_is_named() {
    let rig = common::Rig::start().await;

    // Unknown sessions: every join fails, so join_success cannot hold.
    let mut scenario = join_scenario(vec![4242], 2, 5, 1.0, 0.25);
    scenario.hub.lobby = rig.lobby.addr();
    scenario.hub.edge = rig.edge.edge_addr();

    let report = runner::run(&scenario).await;
    assert_eq!(report.verdict, "fail");
    let join_check = report
        .checks
        .iter()
        .find(|c| c.name == "join_success")
        .expect("join_success checked");
    assert!(!join_check.pass);
    assert!(report.bots.iter().all(|b| b.join_error.is_some()));

    rig.stop().await;
}

/// (conformance: Run report) The report pinpoints per-bot failure detail.
#[tokio::test]
async fn report_pinpoints_failures() {
    let rig = common::Rig::start().await;
    let session = rig.scripted_session().await;

    // One bot targets a live session, one an unknown id: the report must
    // say which bot failed and why.
    let mut scenario = join_scenario(vec![session, 4242], 2, 5, 0.0, 1.0);
    scenario.hub.lobby = rig.lobby.addr();
    scenario.hub.edge = rig.edge.edge_addr();

    let report = runner::run(&scenario).await;
    let failed = report
        .bots
        .iter()
        .find(|b| b.join_error.is_some())
        .expect("a bot failed");
    assert_eq!(failed.session, 4242, "report names the session");
    assert!(
        failed
            .join_error
            .as_deref()
            .unwrap_or_default()
            .contains("4242")
    );

    rig.stop().await;
}

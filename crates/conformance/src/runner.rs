//! The runner: executes a scenario and produces a report.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::info;

use crate::bot::{Bot, BotConfig, Packet};
use crate::report::{Aggregate, BotReport, Check, Report};
use crate::scenario::Scenario;

/// Executes a scenario against the hub it names. The returned report's
/// verdict is the pass/fail decision; the caller decides exit codes.
pub async fn run(scenario: &Scenario) -> Report {
    let sessions = source_sessions(scenario).await;
    let sessions = match sessions {
        Ok(sessions) if !sessions.is_empty() => sessions,
        Ok(_) => return failed_unreachable("no sessions to join (empty id list)"),
        Err(reason) => return failed_unreachable(&reason),
    };

    let deadline = tokio::time::Instant::now()
        + Duration::from_millis(scenario.script.duration_ms.clamp(1_000, 120_000));
    let stall_count = scenario.bots.stall_after_join.min(scenario.bots.count);

    let mut handles = Vec::new();
    for bot_index in 0..scenario.bots.count {
        let cfg = BotConfig {
            lobby: scenario.hub.lobby,
            edge: scenario.hub.edge,
            token: scenario.bots.token.clone(),
        };
        let session = sessions[bot_index as usize % sessions.len()];
        let stalled = (bot_index) < stall_count;
        let inputs = scenario.script.inputs_per_bot;
        let interval = Duration::from_millis(scenario.script.input_interval_ms.max(1));
        handles.push(tokio::spawn(run_bot(
            bot_index as usize,
            cfg,
            session,
            stalled,
            inputs,
            interval,
            deadline,
        )));
    }

    let mut bots = Vec::new();
    for handle in handles {
        match handle.await {
            Ok(report) => bots.push(report),
            Err(err) => bots.push(BotReport {
                bot: usize::MAX,
                session: 0,
                client_id: None,
                join_error: Some(format!("bot task panicked: {err}")),
                stalled: false,
                events: Vec::new(),
                echoes: Vec::new(),
                ordering_violations: 0,
                protocol_violations: 0,
                loss_ratio: 1.0,
                latency_samples_ms: Vec::new(),
                first_divergence: None,
            }),
        }
    }
    bots.sort_by_key(|b| b.bot);

    let report = summarize(scenario, bots);
    info!(
        verdict = report.verdict,
        joins = ?(report.aggregate.joins_ok, report.aggregate.joins_failed),
        "scenario run complete"
    );
    report
}

/// Resolves the session list: created via the lobby API or joined by id.
async fn source_sessions(scenario: &Scenario) -> Result<Vec<u32>, String> {
    match scenario.sessions.mode.as_str() {
        "join" => Ok(scenario.sessions.ids.clone()),
        "create" => {
            let game = scenario.sessions.game.clone().unwrap_or_default();
            let mut created = Vec::new();
            for _ in 0..scenario.sessions.count {
                let body = serde_json::json!({ "game_type": game }).to_string();
                let (status, response) = lobby_post(
                    scenario.hub.lobby,
                    "/api/sessions",
                    &body,
                    &scenario.hub.lobby_token,
                )
                .await?;
                if status != 201 {
                    return Err(format!("session creation failed ({status}): {response}"));
                }
                let id = serde_json::from_str::<serde_json::Value>(&response)
                    .map_err(|e| format!("bad creation response: {e}"))?
                    .get("session_id")
                    .and_then(|v| v.as_u64())
                    .ok_or("creation response has no session_id")? as u32;
                created.push(id);
            }
            Ok(created)
        }
        other => Err(format!("unknown sessions mode {other:?}")),
    }
}

/// One minimal HTTP POST against the lobby.
async fn lobby_post(
    lobby: SocketAddr,
    path: &str,
    body: &str,
    token: &str,
) -> Result<(u16, String), String> {
    let mut stream = tokio::net::TcpStream::connect(lobby)
        .await
        .map_err(|e| format!("lobby unreachable: {e}"))?;
    let mut request = format!(
        "POST {path} HTTP/1.1\r\nHost: hub\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        body.len()
    );
    if !token.is_empty() {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    request.push_str("Connection: close\r\n\r\n");
    request.push_str(body);
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|e| format!("lobby request failed: {e}"))?;
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut buf))
        .await
        .map_err(|_| "lobby response timed out".to_string())?
        .map_err(|e| format!("lobby response failed: {e}"))?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse::<u16>().ok())
        .ok_or("malformed lobby response")?;
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_owned())
        .unwrap_or_default();
    Ok((status, body))
}

/// Drives one bot through its script and measures everything it sees.
#[allow(clippy::too_many_arguments)]
async fn run_bot(
    bot_index: usize,
    cfg: BotConfig,
    session: u32,
    stalled: bool,
    inputs: u32,
    interval: Duration,
    deadline: tokio::time::Instant,
) -> BotReport {
    let mut report = BotReport {
        bot: bot_index,
        session,
        client_id: None,
        join_error: None,
        stalled,
        events: Vec::new(),
        echoes: Vec::new(),
        ordering_violations: 0,
        protocol_violations: 0,
        loss_ratio: 1.0,
        latency_samples_ms: Vec::new(),
        first_divergence: None,
    };

    let mut bot = match Bot::join(&cfg, session, &format!("bot-{bot_index}")).await {
        Ok(bot) => bot,
        Err(reason) => {
            report.join_error = Some(reason);
            return report;
        }
    };
    report.client_id = Some(bot.client_id);

    // Stalled bots: send everything, read nothing, wait out the run.
    if stalled {
        for i in 0..inputs {
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            let _ = bot.send_input(input_payload(i).as_bytes());
            tokio::time::sleep(interval).await;
        }
        bot.stall(deadline).await;
        return report;
    }

    let mut send_times: Vec<Option<std::time::Instant>> = vec![None; inputs as usize];
    let mut latencies: Vec<f64> = Vec::new();
    let mut last_broadcast: Option<u64> = None;
    let mut sent = 0u32;
    let mut next_send = tokio::time::Instant::now();

    loop {
        let finished_sending = sent >= inputs;
        let echoes_needed = sent as usize;
        let enough_received = report.echoes.len() >= echoes_needed && echoes_needed > 0;
        if (finished_sending && enough_received) || tokio::time::Instant::now() >= deadline {
            break;
        }

        tokio::select! {
            packet = bot.next() => {
                match packet {
                    Some(Packet::State { from, payload }) if from == bot.client_id => {
                        if let Ok(text) = String::from_utf8(payload) {
                            if let Some(index) = parse_input_index(&text)
                                && (index as usize) < send_times.len()
                                && let Some(sent_at) = send_times[index as usize]
                            {
                                latencies.push(sent_at.elapsed().as_secs_f64() * 1000.0);
                            }
                            report.echoes.push(text);
                        }
                    }
                    Some(Packet::State { from: 0, payload }) => {
                        // Broadcast tick counters must never regress.
                        if payload.len() == 8 {
                            let tick = u64::from_le_bytes(payload[..8].try_into().expect("8 bytes"));
                            if let Some(last) = last_broadcast
                                && tick <= last
                            {
                                report.ordering_violations += 1;
                                if report.first_divergence.is_none() {
                                    report.first_divergence =
                                        Some(format!("broadcast counter regressed: {tick} after {last}"));
                                }
                            }
                            last_broadcast = Some(tick);
                        }
                    }
                    Some(Packet::Event { payload, .. }) => {
                        if let Ok(text) = String::from_utf8(payload) {
                            report.events.push(text);
                        }
                    }
                    Some(Packet::State { .. }) => {}
                    None => break,
                }
            }
            _ = tokio::time::sleep_until(next_send), if !finished_sending => {
                if sent < inputs {
                    if bot.send_input(input_payload(sent).as_bytes()).is_ok() {
                        send_times[sent as usize] = Some(std::time::Instant::now());
                    }
                    sent += 1;
                    next_send = tokio::time::Instant::now() + interval;
                }
            }
        }
    }
    report.protocol_violations = bot.violations.len() as u32;
    report.loss_ratio = if sent == 0 {
        0.0
    } else {
        1.0 - report.echoes.len() as f64 / f64::from(sent)
    };
    report.latency_samples_ms = latencies;
    report
}

fn input_payload(index: u32) -> String {
    format!("i{index}")
}

fn parse_input_index(text: &str) -> Option<u32> {
    text.strip_prefix('i')?.parse().ok()
}

/// Folds bot reports into the aggregate and checks thresholds.
fn summarize(scenario: &Scenario, bots: Vec<BotReport>) -> Report {
    let counted: Vec<&BotReport> = bots.iter().filter(|b| !b.stalled).collect();

    let joins_ok = counted.iter().filter(|b| b.join_error.is_none()).count() as u32;
    let joins_failed = bots.iter().filter(|b| b.join_error.is_some()).count() as u32;
    let inputs_sent: u32 = counted
        .iter()
        .map(|b| {
            if b.join_error.is_some() {
                0
            } else {
                scenario.script.inputs_per_bot
            }
        })
        .sum();
    let echoes_received: u32 = counted.iter().map(|b| b.echoes.len() as u32).sum();
    let max_loss_ratio = counted
        .iter()
        .filter(|b| b.join_error.is_none())
        .map(|b| b.loss_ratio)
        .fold(0.0_f64, f64::max);
    let ordering_violations: u32 = counted.iter().map(|b| b.ordering_violations).sum();
    let protocol_violations: u32 = counted.iter().map(|b| b.protocol_violations).sum();

    // Latency percentiles across every sample of every counted bot.
    let mut samples: Vec<f64> = counted
        .iter()
        .flat_map(|b| b.latency_samples_ms.iter().copied())
        .collect();
    samples.sort_by(|a, b| a.partial_cmp(b).expect("finite latencies"));
    let percentile = |p: f64| -> f64 {
        if samples.is_empty() {
            return 0.0;
        }
        let index = ((samples.len() as f64 - 1.0) * p).round() as usize;
        samples[index.min(samples.len() - 1)]
    };
    let p50 = percentile(0.50);
    let p95 = percentile(0.95);

    let aggregate = Aggregate {
        joins_ok,
        joins_failed,
        inputs_sent,
        echoes_received,
        max_loss_ratio,
        ordering_violations: ordering_violations + protocol_violations,
        protocol_violations,
        p50_latency_ms: p50,
        p95_latency_ms: p95,
    };

    // Stalled probes are excluded from both sides of the join-rate
    // fraction: they are measurement instruments, not subjects.
    let join_rate = if counted.is_empty() {
        0.0
    } else {
        f64::from(joins_ok) / counted.len() as f64
    };
    let t = &scenario.thresholds;
    let mut checks = vec![
        check(
            "join_success",
            format!("{join_rate:.3}"),
            format!(">= {}", t.join_success),
            join_rate >= t.join_success,
        ),
        check(
            "max_loss_ratio",
            format!("{max_loss_ratio:.3}"),
            format!("<= {}", t.max_loss_ratio),
            max_loss_ratio <= t.max_loss_ratio,
        ),
        check(
            "ordering_violations",
            ordering_violations.to_string(),
            format!("<= {}", t.ordering_violations),
            ordering_violations <= t.ordering_violations,
        ),
    ];
    if t.p95_latency_ms > 0.0 {
        checks.push(check(
            "p95_latency_ms",
            format!("{p95:.1}"),
            format!("<= {}", t.p95_latency_ms),
            p95 <= t.p95_latency_ms,
        ));
    }

    let verdict = if checks.iter().all(|c| c.pass) {
        "pass"
    } else {
        "fail"
    };
    Report {
        verdict,
        checks,
        aggregate,
        bots,
    }
}

fn check(name: &str, measured: String, threshold: String, pass: bool) -> Check {
    Check {
        name: name.to_owned(),
        measured,
        threshold,
        pass,
    }
}

fn failed_unreachable(reason: &str) -> Report {
    Report {
        verdict: "fail",
        checks: vec![check(
            "hub_reachable",
            reason.to_owned(),
            "sessions available".to_owned(),
            false,
        )],
        aggregate: Aggregate::default(),
        bots: Vec::new(),
    }
}

//! Run reports: what was measured, against which thresholds, with what
//! verdict. One JSON document per run.

use serde::Serialize;

/// The aggregate verdict of a run.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// `pass` or `fail`.
    pub verdict: &'static str,
    /// Threshold checks with measured values.
    pub checks: Vec<Check>,
    /// Aggregate traffic numbers.
    pub aggregate: Aggregate,
    /// Per-bot detail (empty details are omitted from bots' entries).
    pub bots: Vec<BotReport>,
}

/// One threshold check: name, measured value, verdict.
#[derive(Debug, Clone, Serialize)]
pub struct Check {
    /// Threshold name, e.g. `max_loss_ratio`.
    pub name: String,
    /// What the run measured.
    pub measured: String,
    /// The declared bound.
    pub threshold: String,
    /// Whether the check passed.
    pub pass: bool,
}

/// Aggregate measurements.
#[derive(Debug, Clone, Serialize, Default)]
pub struct Aggregate {
    /// Bots that joined successfully.
    pub joins_ok: u32,
    /// Bots that failed to join, with reasons in their reports.
    pub joins_failed: u32,
    /// State inputs sent by all (non-stalled) bots.
    pub inputs_sent: u32,
    /// Echoes received back.
    pub echoes_received: u32,
    /// Per-bot maximum loss ratio observed.
    pub max_loss_ratio: f64,
    /// Ordering violations across all bots.
    pub ordering_violations: u32,
    /// Protocol violations across all bots.
    pub protocol_violations: u32,
    /// Latency percentiles in milliseconds (echo turnaround).
    pub p50_latency_ms: f64,
    /// p95 latency in milliseconds.
    pub p95_latency_ms: f64,
}

/// Per-bot detail.
#[derive(Debug, Clone, Serialize)]
pub struct BotReport {
    /// Bot index in the scenario.
    pub bot: usize,
    /// Session the bot targeted.
    pub session: u32,
    /// Assigned client id, when the join succeeded.
    pub client_id: Option<u32>,
    /// Join failure reason, when the join failed.
    pub join_error: Option<String>,
    /// Whether this bot was a deliberate stall probe (excluded from
    /// aggregates).
    pub stalled: bool,
    /// Event payloads received, in stream order.
    pub events: Vec<String>,
    /// Unicast input payloads echoed back (not necessarily in order:
    /// datagrams are unordered).
    pub echoes: Vec<String>,
    /// Ordering violations specific to this bot.
    pub ordering_violations: u32,
    /// Protocol violations specific to this bot.
    pub protocol_violations: u32,
    /// This bot's loss ratio (1.0 when nothing came back).
    pub loss_ratio: f64,
    /// Input→echo latency samples in milliseconds.
    pub latency_samples_ms: Vec<f64>,
    /// First divergence detail, when one was detected.
    pub first_divergence: Option<String>,
}

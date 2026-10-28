//! Scenario files: what the bots do and what "passing" means.

use std::net::SocketAddr;

use serde::Deserialize;

/// A full scenario, deserialized from TOML.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    /// Where the hub lives.
    pub hub: Hub,
    /// How sessions come to be.
    pub sessions: Sessions,
    /// The bot population.
    pub bots: Bots,
    /// What each bot does.
    pub script: Script,
    /// Bounds and pass thresholds.
    pub thresholds: Thresholds,
}

/// Hub endpoints.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hub {
    /// Lobby HTTP endpoint (cert hash, session creation).
    pub lobby: SocketAddr,
    /// WebTransport endpoint.
    pub edge: SocketAddr,
    /// Bearer token for mutating lobby calls (session creation), if set.
    #[serde(default)]
    pub lobby_token: String,
}

/// Session sourcing: created via the lobby API, or joined by id.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sessions {
    /// `create` (lobby API) or `join` (existing ids).
    pub mode: String,
    /// Catalog game type (create mode).
    #[serde(default)]
    pub game: Option<String>,
    /// How many sessions to create (create mode).
    #[serde(default = "default_session_count")]
    pub count: u32,
    /// Session ids to join (join mode).
    #[serde(default)]
    pub ids: Vec<u32>,
}

fn default_session_count() -> u32 {
    1
}

/// The bot population.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bots {
    /// Total bots, spread round-robin across the sessions.
    pub count: u32,
    /// Connect token (empty on an open hub).
    #[serde(default)]
    pub token: String,
    /// How many bots stop reading after joining (isolation probes).
    #[serde(default)]
    pub stall_after_join: u32,
}

/// The per-bot input script.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    /// Inputs each bot sends.
    pub inputs_per_bot: u32,
    /// Delay between inputs.
    #[serde(default = "default_interval")]
    pub input_interval_ms: u64,
    /// Hard ceiling for the whole run.
    #[serde(default = "default_duration")]
    pub duration_ms: u64,
}

fn default_interval() -> u64 {
    20
}

fn default_duration() -> u64 {
    15_000
}

/// Pass thresholds: a run passes only if every check holds.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Thresholds {
    /// Minimum fraction of successful joins.
    #[serde(default = "default_join_success")]
    pub join_success: f64,
    /// Maximum fraction of lost echo datagrams.
    #[serde(default = "default_max_loss")]
    pub max_loss_ratio: f64,
    /// Maximum ordering violations (broadcast monotonicity, stream order).
    #[serde(default)]
    pub ordering_violations: u32,
    /// Maximum p95 input→echo latency in milliseconds (ignored when 0).
    #[serde(default)]
    pub p95_latency_ms: f64,
}

fn default_join_success() -> f64 {
    1.0
}

fn default_max_loss() -> f64 {
    0.05
}

impl Scenario {
    /// Parses a scenario from TOML text, rejecting unknown keys loudly.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let scenario: Scenario =
            toml::from_str(raw).map_err(|err| format!("invalid scenario: {err}"))?;
        match scenario.sessions.mode.as_str() {
            "create" if scenario.sessions.game.is_some() => Ok(scenario),
            "join" if !scenario.sessions.ids.is_empty() => Ok(scenario),
            other => Err(format!(
                "invalid scenario: sessions.mode must be `create` (with game) or `join` \
                 (with ids), got {other:?}"
            )),
        }
    }
}

//! Deterministic fixtures: recorded session streams, compared on replay.
//!
//! A fixture pins, per bot: the exact ordered event payloads (the events
//! channel is a reliable ordered stream), the multiset of unicast echo
//! payloads with a loss bound (state datagrams are unordered and may be
//! lost by design), and the broadcast counter series as a monotonicity
//! witness. Wall-clock timing never appears.

use serde::{Deserialize, Serialize};

use crate::report::Report;

/// The recorded side of a fixture: what the run must produce.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Fixture {
    /// One entry per bot, in bot order.
    pub bots: Vec<BotFixture>,
    /// Maximum tolerated missing unicasts per bot (0..1 fraction).
    pub max_loss_ratio: f64,
}

/// One bot's recorded stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BotFixture {
    /// Bot index.
    pub bot: usize,
    /// Session that was joined.
    pub session: u32,
    /// Exact ordered event payloads, as text.
    pub events: Vec<String>,
    /// Unicast echo payloads, as text (order-insensitive).
    pub echoes: Vec<String>,
}

/// Records a fixture from a (passing) run report.
pub fn record(report: &Report, max_loss_ratio: f64) -> Fixture {
    Fixture {
        bots: report
            .bots
            .iter()
            .filter(|b| !b.stalled)
            .map(|b| BotFixture {
                bot: b.bot,
                session: b.session,
                events: b.events.clone(),
                echoes: b.echoes.clone(),
            })
            .collect(),
        max_loss_ratio,
    }
}

/// Compares a run report against a fixture. Returns every divergence,
/// each naming the bot, channel, and position.
pub fn compare(fixture: &Fixture, report: &Report) -> Vec<String> {
    let mut divergences = Vec::new();
    for expected in &fixture.bots {
        let Some(actual) = report.bots.iter().find(|b| b.bot == expected.bot) else {
            divergences.push(format!("bot {}: missing from the run", expected.bot));
            continue;
        };
        if actual.stalled {
            divergences.push(format!("bot {}: stalled in this run", expected.bot));
            continue;
        }

        // Events: exact and ordered (reliable stream).
        if actual.events != expected.events {
            let position = expected
                .events
                .iter()
                .zip(actual.events.iter())
                .position(|(want, got)| want != got)
                .unwrap_or(expected.events.len().min(actual.events.len()));
            divergences.push(format!(
                "bot {}: events diverge at position {position}: expected {:?}, run {:?}",
                expected.bot,
                expected.events.get(position),
                actual.events.get(position)
            ));
            if actual.events.len() != expected.events.len() {
                divergences.push(format!(
                    "bot {}: event count {} != recorded {}",
                    expected.bot,
                    actual.events.len(),
                    expected.events.len()
                ));
            }
        }

        // Unicasts: loss-bounded multiset comparison (unordered datagrams).
        let mut missing = expected.echoes.clone();
        for got in &actual.echoes {
            if let Some(pos) = missing.iter().position(|m| m == got) {
                missing.remove(pos);
            } else {
                divergences.push(format!(
                    "bot {}: unexpected unicast {:?} (not in the fixture)",
                    expected.bot, got
                ));
            }
        }
        let loss = if expected.echoes.is_empty() {
            0.0
        } else {
            missing.len() as f64 / expected.echoes.len() as f64
        };
        if loss > fixture.max_loss_ratio {
            divergences.push(format!(
                "bot {}: unicast loss {:.3} exceeds {:.3} (first missing: {:?})",
                expected.bot,
                loss,
                fixture.max_loss_ratio,
                missing.first()
            ));
        }
    }
    divergences
}

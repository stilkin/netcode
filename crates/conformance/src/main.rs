//! `netcode-bots` — run a conformance scenario against a hub.
//!
//! ```text
//! netcode-bots run <scenario.toml> [--report <out.json>]
//! ```
//!
//! Exits 0 when every declared threshold is met, 1 otherwise.

use std::path::PathBuf;

use conformance::{runner, scenario::Scenario};

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    let scenario_path = args.next().unwrap_or_default();
    let mut report_path: Option<PathBuf> = None;
    let rest: Vec<String> = args.collect();
    if rest.first().map(String::as_str) == Some("--report") {
        report_path = rest.get(1).map(PathBuf::from);
    }

    if command != "run" || scenario_path.is_empty() {
        eprintln!("usage: netcode-bots run <scenario.toml> [--report <out.json>]");
        std::process::exit(2);
    }

    let raw = match std::fs::read_to_string(&scenario_path) {
        Ok(raw) => raw,
        Err(err) => {
            eprintln!("netcode-bots: cannot read {scenario_path}: {err}");
            std::process::exit(2);
        }
    };
    let scenario = match Scenario::parse(&raw) {
        Ok(scenario) => scenario,
        Err(err) => {
            eprintln!("netcode-bots: {err}");
            std::process::exit(2);
        }
    };

    let report = runner::run(&scenario).await;
    let json = serde_json::to_string_pretty(&report).expect("report serializes");
    if let Some(path) = report_path
        && let Err(err) = std::fs::write(&path, &json)
    {
        eprintln!("netcode-bots: cannot write {path:?}: {err}");
    }
    println!("{json}");

    std::process::exit(if report.verdict == "pass" { 0 } else { 1 });
}

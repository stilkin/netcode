//! Conformance tooling for the netcode hub: protocol-verified headless
//! clients (bots), a scenario runner, deterministic fixtures, and load
//! smoke profiles.
//!
//! The library is deliberately hub-free — bots are external clients, the
//! same trust path a browser takes (lobby-served cert hash, pinned
//! WebTransport, join handshake). Requirements live in the `conformance`
//! capability spec.

pub mod bot;
pub mod fixture;
pub mod report;
pub mod runner;
pub mod scenario;

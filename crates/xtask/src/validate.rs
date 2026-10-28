//! Artifact self-validation: the conformance bots judge the release before
//! any user receives it.

use std::net::{SocketAddr, TcpListener, UdpSocket};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::run;

/// Validates the static musl build: boot the freshly built `hub` with a
/// temporary config, run the freshly built `netcode-bots` smoke scenario
/// against it end to end.
pub fn validate_static_build(root: &Path, target: &str) -> Result<()> {
    let bin_dir = root.join("target").join(target).join("release");
    validate_hub_and_bots(
        root,
        &bin_dir.join("hub"),
        &bin_dir.join("sample-worker"),
        &bin_dir.join("netcode-bots"),
    )
}

/// Rehearsal path for toolchain-less machines: run the built image, point
/// the host debug `netcode-bots` at it.
pub fn validate_docker_image(root: &Path, tag: &str) -> Result<()> {
    println!("== self-validate (docker image {tag}) ==");
    let lobby = free_tcp_port()?;
    let edge = free_udp_port()?;
    let name = format!("netcode-hub-validate-{}", std::process::id());
    let publish = format!("{lobby}:8080/tcp,{edge}:4433/udp");

    run(
        root,
        "docker",
        &["run", "--rm", "-d", "--name", &name, "-p", &publish, tag],
    )
    .context("docker run failed")?;
    // The host debug bots judge the image.
    let bots = root.join("target/debug/netcode-bots");
    let outcome = validate_against(
        root,
        SocketAddr::from(([127, 0, 0, 1], lobby)),
        SocketAddr::from(([127, 0, 0, 1], edge)),
        &bots,
    );
    let _ = run(root, "docker", &["rm", "-f", &name]);
    outcome
}

fn validate_hub_and_bots(root: &Path, hub: &Path, worker: &Path, bots: &Path) -> Result<()> {
    println!("== self-validate (static build) ==");
    let lobby = free_tcp_port()?;
    let edge = free_udp_port()?;
    let dir = std::env::temp_dir().join(format!("xtask-validate-{}", std::process::id()));
    std::fs::create_dir_all(&dir).context("temp validate dir")?;
    let worker_arg = worker.display().to_string();
    let config = format!(
        "[backplane]\nbind = \"127.0.0.1:0\"\n[client_edge]\nbind = \"127.0.0.1:{edge}\"\n[lobby]\nbind = \"127.0.0.1:{lobby}\"\n[[games]]\ngame_type = \"echo-arena\"\ndisplay_name = \"Echo Arena\"\nmax_players = 16\npayload_schema_version = 1\nworker = \"{worker_arg}\"\nsession_config_json = '{{\"tick_hz\":60}}'\n"
    );
    let config_path = dir.join("hub.toml");
    std::fs::write(&config_path, config).context("write validate config")?;

    let mut hub_process = std::process::Command::new(hub)
        .arg("--config")
        .arg(&config_path)
        .current_dir(&dir)
        .spawn()
        .context("start the release hub binary")?;

    let outcome = validate_against(
        root,
        SocketAddr::from(([127, 0, 0, 1], lobby)),
        SocketAddr::from(([127, 0, 0, 1], edge)),
        bots,
    );
    // Teardown: always kill the hub, report the original outcome.
    let _ = hub_process.kill();
    outcome
}

/// Boots nothing; assumes the hub is (or will be) up, then runs the bots.
fn validate_against(root: &Path, lobby: SocketAddr, edge: SocketAddr, bots: &Path) -> Result<()> {
    // Wait for the lobby to answer before spending the bots' time.
    wait_for_lobby(lobby, Duration::from_secs(10))?;

    let scenario = smoke_scenario(lobby, edge);
    let scenario_path =
        std::env::temp_dir().join(format!("xtask-smoke-{}.toml", std::process::id()));
    std::fs::write(&scenario_path, scenario).context("write smoke scenario")?;

    println!("running netcode-bots smoke scenario");
    run(
        root,
        bots.to_str().expect("utf8 bots path"),
        &["run", scenario_path.to_str().expect("utf8 scenario path")],
    )
    .context("smoke scenario failed — the release is not shippable")
}

fn smoke_scenario(lobby: SocketAddr, edge: SocketAddr) -> String {
    format!(
        r#"
[hub]
lobby = "{lobby}"
edge = "{edge}"

[sessions]
mode = "create"
game = "echo-arena"
count = 2

[bots]
count = 8

[script]
inputs_per_bot = 30
input_interval_ms = 10
duration_ms = 20000

[thresholds]
join_success = 1.0
max_loss_ratio = 0.25
ordering_violations = 0
"#
    )
}

/// Polls the lobby's cert-hash route until it answers.
fn wait_for_lobby(lobby: SocketAddr, timeout: Duration) -> Result<()> {
    use std::io::{Read, Write};
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if let Ok(mut stream) =
            std::net::TcpStream::connect_timeout(&lobby, Duration::from_millis(500))
        {
            let _ = stream
                .write_all(b"GET /cert-hash HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
            let mut buf = [0u8; 128];
            if let Ok(n) = stream.read(&mut buf)
                && n > 0
                && buf.starts_with(b"HTTP/1.1 200")
            {
                println!("hub is up (lobby answered on {lobby})");
                return Ok(());
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    bail!("hub did not come up within {timeout:?} (lobby {lobby})")
}

fn free_tcp_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").context("bind probe")?;
    Ok(listener.local_addr().expect("local addr").port())
}

fn free_udp_port() -> Result<u16> {
    let socket = UdpSocket::bind("127.0.0.1:0").context("bind probe")?;
    Ok(socket.local_addr().expect("local addr").port())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// (release-artifacts: A broken binary stops the release) A hub that
    /// dies immediately must fail validation, not hang it.
    #[test]
    fn dead_hub_fails_validation_promptly() {
        let lobby = free_tcp_port().expect("port");
        let err = wait_for_lobby(
            SocketAddr::from(([127, 0, 0, 1], lobby)),
            Duration::from_millis(300),
        )
        .expect_err("nothing listens there");
        assert!(err.to_string().contains("did not come up"), "err: {err:#}");
    }
}

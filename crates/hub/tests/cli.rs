//! End-to-end checks of the hub binary shell: version reporting, startup
//! logging, and graceful shutdown.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

fn hub() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_hub"));
    // No config file exists here: the hub must boot on defaults.
    cmd.current_dir(std::env::temp_dir());
    cmd
}

/// `hub --version` prints "hub <semver>" and exits 0. (Version reporting)
#[test]
fn version_flag() {
    let out = hub().arg("--version").output().expect("run hub --version");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), format!("hub {}", env!("CARGO_PKG_VERSION")));
}

/// Boot, observe the startup log line, then SIGINT: exit 0.
/// (Configuration loading, Structured logging, Graceful shutdown)
#[cfg(unix)]
#[test]
fn boots_logs_and_shuts_down_cleanly() {
    let mut child = hub()
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn hub with piped stdout");
    let stdout = child.stdout.take().expect("piped stdout");
    let mut lines = BufReader::new(stdout).lines();

    let mut saw_startup = false;
    for line in lines.by_ref() {
        let line = line.expect("read stdout line");
        if line.contains("netcode hub starting") {
            saw_startup = true;
            break;
        }
    }
    assert!(saw_startup, "startup log line was not observed");

    // Graceful stop: the first signal must lead to a clean exit.
    unsafe {
        libc::kill(child.id() as i32, libc::SIGINT);
    }
    let status = child.wait().expect("wait for hub");
    assert_eq!(status.code(), Some(0), "expected a clean exit");

    // And the farewell line should have made it out.
    let stopped = lines.any(|line| {
        line.map(|l| l.contains("netcode hub stopped"))
            .unwrap_or(false)
    });
    assert!(stopped, "shutdown log line was not observed");
}

/// Invalid configuration exits 1 and names the offending key.
/// (Configuration loading)
#[test]
fn invalid_config_exits_1() {
    let dir = std::env::temp_dir().join(format!("hub-cli-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    std::fs::write(dir.join("hub.toml"), "log_lvl = \"info\"\n").expect("write bad config");

    let out = hub()
        .current_dir(&dir)
        .output()
        .expect("run hub with bad config");

    assert_eq!(out.status.code(), Some(1), "expected the config exit code");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("log_lvl"),
        "error should name the key: {stderr}"
    );
}

//! `xtask` — local release tooling (no hosted CI, by standing decision).
//!
//! ```text
//! cargo run -p xtask -- preflight
//! cargo run -p xtask -- release [--publish] [--skip-windows] [--skip-docker]
//! cargo run -p xtask -- set-version <x.y.z>
//! ```

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

mod package;
mod preflight;
mod release;
mod validate;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: xtask <preflight|release|set-version> [flags]";
    let Some(command) = args.first() else {
        eprintln!("{usage}");
        std::process::exit(2);
    };
    let rest = &args[1..];
    let outcome = match command.as_str() {
        "preflight" => preflight::preflight(&release::Flags::parse(rest)),
        "release" => release::release(&release::Flags::parse(rest)),
        "set-version" => set_version(rest),
        other => {
            eprintln!("unknown command {other:?}; {usage}");
            std::process::exit(2);
        }
    };
    if let Err(err) = outcome {
        eprintln!("xtask: {err:#}");
        std::process::exit(1);
    }
}

/// Rewrites the workspace version and reminds the operator to commit.
fn set_version(args: &[String]) -> Result<()> {
    let Some(version) = args.first() else {
        bail!("usage: xtask set-version <x.y.z>");
    };
    // Refuse nonsense versions early.
    semver::parse(version).map_err(|e| anyhow::anyhow!(e))?;

    let root = workspace_root();
    let manifest = root.join("Cargo.toml");
    let raw = std::fs::read_to_string(&manifest).context("read workspace manifest")?;
    let mut doc = raw
        .parse::<toml_edit::DocumentMut>()
        .context("parse workspace manifest")?;
    doc["workspace"]["package"]["version"] = toml_edit::value(version);
    std::fs::write(&manifest, doc.to_string()).context("write workspace manifest")?;
    println!("workspace version -> {version}");
    println!("commit the bump, then run `cargo run -p xtask -- release`");
    Ok(())
}

/// The workspace root: the nearest ancestor directory whose manifest
/// declares a workspace. Robust to the crate's position within it.
pub fn workspace_root() -> PathBuf {
    let start = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for dir in start.ancestors().skip(1) {
        if let Ok(manifest) = std::fs::read_to_string(dir.join("Cargo.toml"))
            && manifest.contains("[workspace]")
        {
            return dir.to_path_buf();
        }
    }
    panic!("no workspace manifest above {}", start.display());
}

/// Tiny semver shape check without pulling a semver crate.
mod semver {
    pub fn parse(version: &str) -> Result<(), String> {
        let parts: Vec<_> = version.split('.').collect();
        if parts.len() == 3
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        {
            Ok(())
        } else {
            Err(format!("{version:?} is not x.y.z"))
        }
    }
}

/// Runs a command, inheriting output, failing loudly with its argv.
pub fn run(cwd: &std::path::Path, program: &str, args: &[&str]) -> Result<()> {
    let status = std::process::Command::new(program)
        .args(args)
        .current_dir(cwd)
        .status()
        .with_context(|| format!("cannot run {program}"))?;
    if !status.success() {
        bail!("{program} {} failed with {status}", args.join(" "));
    }
    Ok(())
}

/// Runs a command capturing stdout, for probing and version reads.
pub fn capture(cwd: &std::path::Path, program: &str, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("cannot run {program}"))?;
    if !output.status.success() {
        bail!(
            "{program} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The workspace version from `[workspace.package]`.
pub fn workspace_version(root: &std::path::Path) -> Result<String> {
    let raw =
        std::fs::read_to_string(root.join("Cargo.toml")).context("read workspace manifest")?;
    let doc = raw
        .parse::<toml_edit::DocumentMut>()
        .context("parse workspace manifest")?;
    doc.get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .with_context(|| "workspace.package.version is missing")
}

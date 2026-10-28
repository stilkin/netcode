//! Preflight: refuse dirty trees, probe tools with install hints.

use anyhow::{Context, Result, bail};

use crate::{capture, workspace_root};

/// One tool probe: required or optional, with an install hint.
struct Tool {
    name: &'static str,
    /// How to check: the binary resolvable on PATH.
    binary: &'static str,
    hint: &'static str,
}

const TOOLS: &[Tool] = &[
    Tool {
        name: "musl-gcc (linux target)",
        binary: "musl-gcc",
        hint: "apt install musl-tools  (Debian/Ubuntu; equivalent elsewhere)",
    },
    Tool {
        name: "mingw-w64 (windows target)",
        binary: "x86_64-w64-mingw32-gcc",
        hint: "apt install mingw-w64  (Debian/Ubuntu; equivalent elsewhere)",
    },
    Tool {
        name: "docker",
        binary: "docker",
        hint: "install docker engine, or pass --skip-docker",
    },
    Tool {
        name: "gh (publishing)",
        binary: "gh",
        hint: "https://cli.github.com/ — only needed with --publish",
    },
];

/// Runs every check a release needs before compiling anything.
pub fn preflight(flags: &crate::release::Flags) -> Result<()> {
    let root = workspace_root();
    println!("== preflight ==");

    // Dirty tree: a release must be a commit, not a guess.
    let dirty = capture(&root, "git", &["status", "--porcelain"])
        .context("git status failed — not a git checkout?")?;
    if !dirty.is_empty() {
        let files: Vec<&str> = dirty.lines().take(10).collect();
        bail!(
            "working tree is dirty:\n  {}\n{}commit or stash first",
            files.join("\n  "),
            if dirty.lines().count() > 10 {
                "…\n  "
            } else {
                ""
            }
        );
    }
    println!("clean tree");

    for tool in TOOLS {
        let present = std::process::Command::new(tool.binary)
            .arg("--version")
            .output()
            .is_ok_and(|out| out.status.success());
        let needed = match tool.binary {
            "musl-gcc" => !flags.skip_linux,
            "x86_64-w64-mingw32-gcc" => !flags.skip_windows,
            "docker" => !flags.skip_docker,
            "gh" => flags.publish,
            _ => false,
        };
        if present {
            println!("{}: ok", tool.name);
        } else if needed {
            bail!("missing {}: install with: {}", tool.name, tool.hint);
        } else {
            println!("{}: missing (skipped by flags)", tool.name);
        }
    }
    Ok(())
}

/// Supply-chain check: `cargo audit` when present, honest when not.
pub fn audit(root: &std::path::Path) -> Result<()> {
    let installed = std::process::Command::new("cargo")
        .arg("audit")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success());
    if !installed {
        println!("cargo audit: not installed — supply-chain check SKIPPED");
        println!("  (cargo install cargo-audit to enable)");
        return Ok(());
    }
    println!("== cargo audit ==");
    crate::run(root, "cargo", &["audit"])?;
    Ok(())
}

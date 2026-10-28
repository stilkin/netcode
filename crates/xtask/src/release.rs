//! Release flags and the release flow: preflight → cross-build → package →
//! image → self-validate → (opt-in) publish.

use anyhow::{Context, Result, bail};

use crate::{capture, run, workspace_root, workspace_version};

/// What the release should do (and skip).
#[derive(Debug, Default, Clone)]
pub struct Flags {
    /// Publish: git tag, gh release, docker push (after validation).
    pub publish: bool,
    /// Skip the Windows cross-build (e.g. no mingw on this machine).
    pub skip_windows: bool,
    /// Skip the Docker image build (e.g. no docker).
    pub skip_docker: bool,
    /// Skip the Linux musl build (rehearsals on toolchain-less machines).
    pub skip_linux: bool,
}

impl Flags {
    pub fn parse(args: &[String]) -> Self {
        let mut flags = Flags::default();
        for arg in args {
            match arg.as_str() {
                "--publish" => flags.publish = true,
                "--skip-windows" => flags.skip_windows = true,
                "--skip-docker" => flags.skip_docker = true,
                "--skip-linux" => flags.skip_linux = true,
                other => {
                    eprintln!("xtask: unknown flag {other:?}");
                    std::process::exit(2);
                }
            }
        }
        flags
    }
}

/// One release. Refuses dirty trees; probes tools before compiling.
pub fn release(flags: &Flags) -> Result<()> {
    let root = workspace_root();
    let version = workspace_version(&root)?;

    // --- preflight ---
    super::preflight::preflight(flags)?;

    println!("== release {version} ==");
    if flags.skip_linux {
        println!("--skip-linux: static Linux binaries AND self-validation skipped");
    }

    // --- cross-builds ---
    let mut artifacts: Vec<std::path::PathBuf> = Vec::new();
    let out_dir = root.join("target/release-artifacts").join(&version);
    std::fs::create_dir_all(&out_dir).context("create artifacts dir")?;

    if !flags.skip_linux {
        build_target(&root, "x86_64-unknown-linux-musl")?;
        verify_static(&root, "x86_64-unknown-linux-musl", "hub")?;
        let dir = binaries(&root, "x86_64-unknown-linux-musl");
        artifacts.push(crate::package::package_linux(
            &dir,
            &out_dir,
            &version,
            "x86_64-unknown-linux-musl",
        )?);
    }
    if !flags.skip_windows {
        build_target(&root, "x86_64-pc-windows-gnu")?;
        let dir = binaries(&root, "x86_64-pc-windows-gnu");
        artifacts.push(crate::package::package_windows(
            &dir,
            &out_dir,
            &version,
            "x86_64-pc-windows-gnu",
        )?);
    }
    let checksums = crate::package::write_checksums(&artifacts, &out_dir)?;
    println!("packaged {} artifacts + {checksums:?}", artifacts.len());

    // --- docker image ---
    let mut image: Option<String> = None;
    if !flags.skip_docker {
        let tag = format!("netcode-hub:{version}");
        println!("building docker image {tag}");
        run(&root, "docker", &["build", "-t", &tag, "."]).context("docker build failed")?;
        image = Some(tag);
    }

    // --- self-validation: the bots judge the artifact before users do ---
    if !flags.skip_linux {
        crate::validate::validate_static_build(&root, "x86_64-unknown-linux-musl")?;
    } else if let Some(tag) = &image {
        // Rehearsal path for toolchain-less machines: validate the image.
        crate::validate::validate_docker_image(&root, tag)?;
    }

    // --- supply chain ---
    super::preflight::audit(&root)?;

    println!("== release {version} complete ==");
    for artifact in &artifacts {
        println!("  {}", artifact.display());
    }
    if let Some(tag) = &image {
        println!("  image {tag}");
    }

    // --- publish (opt-in, post-validation) ---
    if flags.publish {
        publish(&root, &version, &artifacts, image.as_deref())?;
    } else {
        println!("(dry run: pass --publish to tag, upload, and push)");
    }
    Ok(())
}

fn build_target(root: &std::path::Path, target: &str) -> Result<()> {
    println!("building {target} (hub, sample-worker, netcode-bots)");
    run(
        root,
        "cargo",
        &[
            "build",
            "--release",
            "--target",
            target,
            "-p",
            "hub",
            "-p",
            "sample-worker",
            "-p",
            "conformance",
        ],
    )
    .with_context(|| format!("build for {target}"))
}

/// The three binaries of one target's release directory.
fn binaries(root: &std::path::Path, target: &str) -> std::path::PathBuf {
    root.join("target").join(target).join("release")
}

/// The musl build must be genuinely static.
fn verify_static(root: &std::path::Path, target: &str, binary: &str) -> Result<()> {
    let path = binaries(root, target).join(binary);
    let output = std::process::Command::new("ldd")
        .arg(&path)
        .output()
        .context("run ldd")?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !text.contains("not a dynamic executable") && !text.contains("statically linked") {
        bail!("{binary} ({target}) is not static: {text}");
    }
    println!("{binary} is static");
    Ok(())
}

/// The publish phase: tag, upload, push. Only after validation passed.
fn publish(
    root: &std::path::Path,
    version: &str,
    artifacts: &[std::path::PathBuf],
    image: Option<&str>,
) -> Result<()> {
    let tag = format!("v{version}");
    let existing = capture(root, "git", &["tag", "--list", &tag]).unwrap_or_default();
    if existing.is_empty() {
        println!("publish: creating git tag {tag}");
        run(root, "git", &["tag", &tag])?;
    } else {
        println!("publish: git tag {tag} already exists, keeping it");
    }

    println!("publish: creating GitHub release {tag}");
    let mut args = vec![
        "release".to_owned(),
        "create".to_owned(),
        tag.clone(),
        "--title".to_owned(),
        tag.clone(),
    ];
    for artifact in artifacts {
        args.push(artifact.display().to_string());
    }
    args.push(format!(
        "{}/SHA256SUMS",
        root.join("target/release-artifacts")
            .join(version)
            .display()
    ));
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run(root, "gh", &arg_refs).context("gh release create failed")?;

    if let Some(tag) = image {
        println!("publish: docker push {tag}");
        run(root, "docker", &["push", tag]).context("docker push failed")?;
    }
    println!("publish: done");
    Ok(())
}

//! Packaging: per-target archives plus a SHA256SUMS file.
//!
//! One deployment unit per archive: `hub`, `sample-worker`, `netcode-bots`
//! and an example `hub.toml` — the hub and its workers ship together.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

/// The example configuration shipped in every archive (and the Docker
/// image): a catalog pointing at the co-installed sample worker.
pub const EXAMPLE_CONFIG: &str = r#"# Example configuration for a released netcode hub.
# Docs: https://github.com/stilkin/netcode — see README.md for every key.

[client_edge]
bind = "0.0.0.0:4433"     # WebTransport (browsers, UDP)

[lobby]
bind = "0.0.0.0:8080"     # HTTP: lobby API, cert hash, test page
#token = "s3cret"         # uncomment to require it for mutations

[[games]]
game_type = "echo-arena"
display_name = "Echo Arena"
max_players = 8
payload_schema_version = 1
worker = "sample-worker"  # relative to the hub's working directory
session_config_json = '{"tick_hz":60}'
"#;

/// Binaries one archive carries, with their Windows names.
const LINUX_BINARIES: &[&str] = &["hub", "sample-worker", "netcode-bots"];
const WINDOWS_BINARIES: &[&str] = &["hub.exe", "sample-worker.exe", "netcode-bots.exe"];

/// Packages the Linux target as `<name>.tar.gz`.
pub fn package_linux(
    bin_dir: &Path,
    out_dir: &Path,
    version: &str,
    target: &str,
) -> Result<PathBuf> {
    let archive = out_dir.join(format!("netcode-hub-{version}-{target}.tar.gz"));
    let file = std::fs::File::create(&archive).context("create tar.gz")?;
    let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut tar = tar::Builder::new(enc);
    for binary in LINUX_BINARIES {
        let path = bin_dir.join(binary);
        std::fs::metadata(&path).with_context(|| format!("missing {binary} in {target} build"))?;
        tar.append_path_with_name(&path, binary)
            .with_context(|| format!("append {binary}"))?;
    }
    let mut header = tar::Header::new_gnu();
    header.set_size(EXAMPLE_CONFIG.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(&mut header, "hub.toml", EXAMPLE_CONFIG.as_bytes())
        .context("append hub.toml")?;
    tar.into_inner()
        .context("finish tar")?
        .finish()
        .context("finish gzip")?;
    Ok(archive)
}

/// Packages the Windows target as `<name>.zip`.
pub fn package_windows(
    bin_dir: &Path,
    out_dir: &Path,
    version: &str,
    target: &str,
) -> Result<PathBuf> {
    let archive = out_dir.join(format!("netcode-hub-{version}-{target}.zip"));
    let file = std::fs::File::create(&archive).context("create zip")?;
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for binary in WINDOWS_BINARIES {
        let path = bin_dir.join(binary);
        let bytes =
            std::fs::read(&path).with_context(|| format!("missing {binary} in {target} build"))?;
        zip.start_file(*binary, options)
            .with_context(|| format!("zip {binary}"))?;
        zip.write_all(&bytes)?;
    }
    zip.start_file("hub.toml", options)?;
    zip.write_all(EXAMPLE_CONFIG.as_bytes())?;
    zip.finish().context("finish zip")?;
    Ok(archive)
}

/// Writes SHA256SUMS covering every artifact, next to them.
pub fn write_checksums(artifacts: &[PathBuf], out_dir: &Path) -> Result<PathBuf> {
    let sums = out_dir.join("SHA256SUMS");
    let mut body = String::new();
    for artifact in artifacts {
        let digest = sha256_file(artifact)?;
        body.push_str(&format!(
            "{digest}  {}\n",
            artifact
                .file_name()
                .expect("artifact has a name")
                .to_string_lossy()
        ));
    }
    std::fs::write(&sums, body).context("write SHA256SUMS")?;
    Ok(sums)
}

/// SHA-256 of a file, hex-encoded.
pub fn sha256_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("xtask-pkg-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn fake_binaries(dir: &Path, names: &[&str]) {
        for name in names {
            std::fs::write(dir.join(name), format!("fake {name}")).expect("fake binary");
        }
    }

    #[test]
    fn linux_archive_contains_binaries_and_config() {
        let dir = scratch("linux");
        fake_binaries(&dir, LINUX_BINARIES);
        let archive = package_linux(&dir, &dir, "9.9.9", "x86_64-test").expect("package");
        let bytes = std::fs::read(&archive).expect("read archive");
        let dec = flate2::read::GzDecoder::new(&bytes[..]);
        let mut tar = tar::Archive::new(dec);
        let mut names: Vec<String> = tar
            .entries()
            .expect("entries")
            .map(|e| {
                e.expect("entry")
                    .path()
                    .expect("path")
                    .display()
                    .to_string()
            })
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec!["hub", "hub.toml", "netcode-bots", "sample-worker"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn windows_archive_contains_binaries_and_config() {
        let dir = scratch("windows");
        fake_binaries(&dir, WINDOWS_BINARIES);
        let archive = package_windows(&dir, &dir, "9.9.9", "x86_64-test").expect("package");
        let file = std::fs::File::open(&archive).expect("open");
        let zip = zip::ZipArchive::new(file).expect("zip");
        let mut names: Vec<String> = zip
            .file_names()
            .map(|n| n.expect("zip name").into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "hub.exe",
                "hub.toml",
                "netcode-bots.exe",
                "sample-worker.exe"
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn checksums_match_recomputed_digests() {
        let dir = scratch("sums");
        fake_binaries(&dir, &["a", "b"]);
        let artifacts = vec![dir.join("a"), dir.join("b")];
        let sums = write_checksums(&artifacts, &dir).expect("sums");
        let body = std::fs::read_to_string(&sums).expect("read sums");
        for artifact in &artifacts {
            let digest = sha256_file(artifact).expect("digest");
            assert!(
                body.contains(&digest),
                "SHA256SUMS covers {}",
                artifact.display()
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_binaries_fail_with_names() {
        let dir = scratch("missing");
        let err = package_linux(&dir, &dir, "1.0.0", "t").expect_err("must fail");
        assert!(err.to_string().contains("hub"), "names the binary: {err:#}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

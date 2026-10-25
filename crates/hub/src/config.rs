//! Hub configuration (`hub.toml`).

use std::fmt;
use std::path::Path;

use serde::Deserialize;

/// Top-level hub configuration.
///
/// Unknown keys are rejected so a typo fails loudly instead of silently
/// falling back to a default. Later changes extend this struct (backplane
/// address, client ports, worker paths).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    /// Log level: one of `trace`, `debug`, `info`, `warn`, `error`.
    pub log_level: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            log_level: "info".to_owned(),
        }
    }
}

/// A configuration problem, ready to show the operator.
#[derive(Debug)]
pub struct ConfigError(pub String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "config error: {}", self.0)
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    /// Loads configuration from `path`. A missing file means all defaults.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            // A missing file is fine: defaults exist so the shell boots bare.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => {
                return Err(ConfigError(format!(
                    "cannot read {}: {err}",
                    path.display()
                )));
            }
        };
        let config: Self = toml::from_str(&raw)
            .map_err(|err| ConfigError(format!("invalid config in {}: {err}", path.display())))?;
        if let Err(err) = config
            .log_level
            .parse::<tracing::level_filters::LevelFilter>()
        {
            return Err(ConfigError(format!(
                "invalid config in {}: `log_level` = {:?} is not a valid level: {err}",
                path.display(),
                config.log_level
            )));
        }
        Ok(config)
    }
}

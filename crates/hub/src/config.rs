//! Hub configuration (`hub.toml`).

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Top-level hub configuration.
///
/// Unknown keys are rejected so a typo fails loudly instead of silently
/// falling back to a default. Later changes extend this struct (client ports,
/// worker paths).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    /// Log level: one of `trace`, `debug`, `info`, `warn`, `error`.
    pub log_level: String,
    /// Hub-to-worker listener settings.
    pub backplane: BackplaneConfig,
    /// Browser-facing WebTransport listener settings.
    pub client_edge: ClientEdgeConfig,
    /// Lobby/admin HTTP API settings.
    pub lobby: LobbyConfig,
    /// The game catalog: one `[[games]]` table per installable game.
    #[serde(default)]
    pub games: Vec<GameConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            log_level: "info".to_owned(),
            backplane: BackplaneConfig::default(),
            client_edge: ClientEdgeConfig::default(),
            lobby: LobbyConfig::default(),
            games: Vec::new(),
        }
    }
}

/// Backplane listener settings.
///
/// The bind address is loopback by design: the backplane is a trusted,
/// same-machine control plane, not an internet-facing port.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct BackplaneConfig {
    /// Address the listener binds to, e.g. `127.0.0.1:24000`.
    pub bind: String,
    /// Shared token workers must present in `Hello`. Empty disables the
    /// check (loopback trust).
    pub token: String,
}

impl Default for BackplaneConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:24000".to_owned(),
            token: String::new(),
        }
    }
}

/// Browser-facing client-edge listener settings (WebTransport + bootstrap).
///
/// Unlike the backplane these bind all interfaces: the client edge is the
/// LAN-facing surface players connect to.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ClientEdgeConfig {
    /// WebTransport (QUIC/UDP) listener address, e.g. `0.0.0.0:4433`.
    pub bind: String,
    /// Shared secret a `Connect` frame must present. Empty disables the
    /// check (LAN trust), mirroring the backplane token.
    pub token: String,
}

impl Default for ClientEdgeConfig {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0:4433".to_owned(),
            token: String::new(),
        }
    }
}

/// Lobby/admin HTTP API settings.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LobbyConfig {
    /// HTTP listener address, e.g. `0.0.0.0:8080`.
    pub bind: String,
    /// Shared secret guarding mutating operations. Empty disables the
    /// check (LAN trust), mirroring the backplane and client edge tokens.
    pub token: String,
}

impl Default for LobbyConfig {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0:8080".to_owned(),
            token: String::new(),
        }
    }
}

/// One `[[games]]` catalog entry: how the hub starts this game's workers.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameConfig {
    /// Unique catalog key, e.g. `echo-arena`.
    pub game_type: String,
    /// Human-readable name for directory UIs.
    pub display_name: String,
    /// Default player capacity for sessions of this game.
    pub max_players: u16,
    /// Payload schema version this game's snapshots/events speak.
    pub payload_schema_version: u16,
    /// Worker program the hub spawns per session.
    pub worker: PathBuf,
    /// Session configuration JSON handed to the worker (`--session-config`).
    #[serde(default = "default_session_config")]
    pub session_config_json: String,
}

fn default_session_config() -> String {
    "{}".to_owned()
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
        // Retired keys get a pointing error, not a bare unknown-field error.
        if let Ok(table) = toml::from_str::<toml::Table>(&raw)
            && table
                .get("client_edge")
                .and_then(|edge| edge.get("bootstrap_bind"))
                .is_some()
        {
            return Err(ConfigError(format!(
                "invalid config in {}: `client_edge.bootstrap_bind` is retired — the bootstrap endpoint moved to the lobby API; use `[lobby] bind`",
                path.display()
            )));
        }
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
        if let Err(err) = config.backplane.bind.parse::<std::net::SocketAddr>() {
            return Err(ConfigError(format!(
                "invalid config in {}: `backplane.bind` = {:?} is not a valid address: {err}",
                path.display(),
                config.backplane.bind
            )));
        }
        for (key, value) in [
            ("client_edge.bind", &config.client_edge.bind),
            ("lobby.bind", &config.lobby.bind),
        ] {
            if let Err(err) = value.parse::<std::net::SocketAddr>() {
                return Err(ConfigError(format!(
                    "invalid config in {}: `{key}` = {:?} is not a valid address: {err}",
                    path.display(),
                    value
                )));
            }
        }
        let mut seen = HashSet::new();
        for game in &config.games {
            if !seen.insert(game.game_type.clone()) {
                return Err(ConfigError(format!(
                    "invalid config in {}: duplicate `game_type` {:?} in `[[games]]`",
                    path.display(),
                    game.game_type
                )));
            }
        }
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn write_tmp(name: &str, contents: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("hub-cfg-test-{}-{name}", std::process::id()));
        std::fs::write(&path, contents).expect("write temp config");
        path
    }

    /// A missing file means all defaults. (Configuration loading)
    #[test]
    fn missing_file_means_defaults() {
        let config = Config::load(Path::new("/nonexistent/hub.toml")).expect("defaults");
        assert_eq!(config, Config::default());
        assert_eq!(config.log_level, "info");
    }

    /// A valid file parses. (Configuration loading)
    #[test]
    fn valid_file_parses() {
        let path = write_tmp("valid", "log_level = \"debug\"\n");
        let config = Config::load(&path).expect("valid config");
        assert_eq!(config.log_level, "debug");
        std::fs::remove_file(path).ok();
    }

    /// An unknown key is rejected and named. (Configuration loading)
    #[test]
    fn unknown_key_names_the_key() {
        let path = write_tmp("unknown-key", "log_lvl = \"info\"\n");
        let err = Config::load(&path).expect_err("typo must fail");
        assert!(err.to_string().contains("log_lvl"), "error: {err}");
    }

    /// An invalid level is rejected and named. (Configuration loading)
    #[test]
    fn invalid_level_names_the_key() {
        let path = write_tmp("bad-level", "log_level = \"loud\"\n");
        let err = Config::load(&path).expect_err("bad level must fail");
        let msg = err.to_string();
        assert!(
            msg.contains("log_level") && msg.contains("loud"),
            "error: {msg}"
        );
    }
}

#[cfg(test)]
mod backplane_config_tests {
    use super::*;

    fn write_tmp(name: &str, contents: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("hub-cfg-test-{}-{name}", std::process::id()));
        std::fs::write(&path, contents).expect("write temp config");
        path
    }

    /// (hub-runtime: Configuration loading) Defaults include the backplane.
    #[test]
    fn defaults_include_backplane() {
        let config = Config::load(Path::new("/nonexistent/hub.toml")).expect("defaults");
        assert_eq!(config.backplane.bind, "127.0.0.1:24000");
        assert_eq!(config.backplane.token, "");
    }

    /// (hub-runtime: Configuration loading) A valid section parses.
    #[test]
    fn backplane_section_parses() {
        let path = write_tmp(
            "backplane-valid",
            "log_level = \"debug\"\n[backplane]\nbind = \"127.0.0.1:24111\"\ntoken = \"s3cret\"\n",
        );
        let config = Config::load(&path).expect("valid config");
        assert_eq!(config.backplane.bind, "127.0.0.1:24111");
        assert_eq!(config.backplane.token, "s3cret");
    }

    /// (hub-runtime: Configuration loading) Unknown nested keys are rejected.
    #[test]
    fn unknown_backplane_key_is_rejected() {
        let path = write_tmp(
            "backplane-typo",
            "[backplane]\nprot = \"127.0.0.1:24000\"\n",
        );
        let err = Config::load(&path).expect_err("typo must fail");
        assert!(err.to_string().contains("prot"), "error: {err}");
    }

    /// (hub-runtime: Configuration loading) An invalid bind address is rejected.
    #[test]
    fn invalid_bind_is_rejected() {
        let path = write_tmp("backplane-bind", "[backplane]\nbind = \"nonsense\"\n");
        let err = Config::load(&path).expect_err("bad bind must fail");
        assert!(err.to_string().contains("backplane.bind"), "error: {err}");
    }
}

#[cfg(test)]
mod client_edge_config_tests {
    use super::*;
    use std::path::Path;

    fn write_tmp(name: &str, contents: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("hub-cfg-test-{}-{name}", std::process::id()));
        std::fs::write(&path, contents).expect("write temp config");
        path
    }

    /// (hub-runtime: Client edge startup) Defaults without the section.
    #[test]
    fn defaults_include_client_edge() {
        let config = Config::load(Path::new("/nonexistent/hub.toml")).expect("defaults");
        assert_eq!(config.client_edge.bind, "0.0.0.0:4433");
        assert_eq!(config.client_edge.token, "");
    }

    /// (hub-runtime: Client edge startup) A valid section parses.
    #[test]
    fn client_edge_section_parses() {
        let path = write_tmp(
            "edge-valid",
            "[client_edge]\nbind = \"0.0.0.0:4533\"\ntoken = \"lan\"\n",
        );
        let config = Config::load(&path).expect("valid config");
        assert_eq!(config.client_edge.bind, "0.0.0.0:4533");
        assert_eq!(config.client_edge.token, "lan");
    }

    /// (hub-runtime: Lobby configuration) The retired bootstrap key points
    /// operators at its replacement.
    #[test]
    fn retired_bootstrap_bind_names_the_replacement() {
        let path = write_tmp(
            "edge-retired",
            "[client_edge]\nbind = \"0.0.0.0:4433\"\nbootstrap_bind = \"0.0.0.0:8080\"\n",
        );
        let err = Config::load(&path).expect_err("retired key must fail loudly");
        let msg = err.to_string();
        assert!(
            msg.contains("bootstrap_bind") && msg.contains("[lobby] bind"),
            "error: {msg}"
        );
    }

    /// (hub-runtime: Client edge startup) Unknown nested keys are rejected
    /// and named.
    #[test]
    fn unknown_client_edge_key_is_rejected() {
        let path = write_tmp("edge-typo", "[client_edge]\nprotocl = \"webtransport\"\n");
        let err = Config::load(&path).expect_err("typo must fail");
        assert!(err.to_string().contains("protocl"), "error: {err}");
    }

    /// (hub-runtime: Client edge startup) Invalid addresses are rejected
    /// and named.
    #[test]
    fn invalid_client_edge_bind_is_rejected() {
        let path = write_tmp("edge-bind", "[client_edge]\nbind = \"nonsense\"\n");
        let err = Config::load(&path).expect_err("bad bind must fail");
        assert!(err.to_string().contains("client_edge.bind"), "error: {err}");

        let path = write_tmp("lobby-bind", "[lobby]\nbind = \"nonsense\"\n");
        let err = Config::load(&path).expect_err("bad bind must fail");
        assert!(err.to_string().contains("lobby.bind"), "error: {err}");
    }
}

#[cfg(test)]
mod lobby_config_tests {
    use super::*;
    use std::path::Path;

    fn write_tmp(name: &str, contents: impl AsRef<str>) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("hub-cfg-test-{}-{name}", std::process::id()));
        std::fs::write(&path, contents.as_ref()).expect("write temp config");
        path
    }

    const ONE_GAME: &str = "[[games]]\ngame_type = \"echo-arena\"\ndisplay_name = \"Echo Arena\"\nmax_players = 8\npayload_schema_version = 1\nworker = \"/opt/games/sample-worker\"\n";

    /// (hub-runtime: Lobby configuration) Defaults: empty catalog, standard
    /// port, open LAN.
    #[test]
    fn lobby_defaults() {
        let config = Config::load(Path::new("/nonexistent/hub.toml")).expect("defaults");
        assert_eq!(config.lobby.bind, "0.0.0.0:8080");
        assert_eq!(config.lobby.token, "");
        assert!(config.games.is_empty(), "no games by default");
    }

    /// (hub-runtime: Lobby configuration) Sections parse, session config
    /// defaults to `{}`.
    #[test]
    fn lobby_and_games_parse() {
        let path = write_tmp(
            "lobby-valid",
            "[lobby]\nbind = \"0.0.0.0:9090\"\ntoken = \"s3cret\"\n".to_owned() + ONE_GAME,
        );
        let config = Config::load(&path).expect("valid config");
        assert_eq!(config.lobby.bind, "0.0.0.0:9090");
        assert_eq!(config.lobby.token, "s3cret");
        assert_eq!(config.games.len(), 1);
        assert_eq!(config.games[0].game_type, "echo-arena");
        assert_eq!(
            config.games[0].worker,
            PathBuf::from("/opt/games/sample-worker")
        );
        assert_eq!(config.games[0].session_config_json, "{}");
    }

    /// (hub-runtime: Lobby configuration) Duplicate game types fail loudly.
    #[test]
    fn duplicate_game_type_is_rejected() {
        let path = write_tmp("games-dupe", ONE_GAME.to_owned() + ONE_GAME);
        let err = Config::load(&path).expect_err("duplicate must fail");
        assert!(
            err.to_string().contains("duplicate") && err.to_string().contains("echo-arena"),
            "error: {err}"
        );
    }

    /// (hub-runtime: Lobby configuration) Unknown lobby keys are named.
    #[test]
    fn unknown_lobby_key_is_rejected() {
        let path = write_tmp("lobby-typo", "[lobby]\nbind_adress = \"x\"\n");
        let err = Config::load(&path).expect_err("typo must fail");
        assert!(err.to_string().contains("bind_adress"), "error: {err}");
    }

    /// (hub-runtime: Lobby configuration) Unknown game keys are named.
    #[test]
    fn unknown_game_key_is_rejected() {
        let path = write_tmp(
            "games-typo",
            "[[games]]\ngame_type = \"x\"\ndisplay_name = \"X\"\nmax_players = 2\npayload_schema_version = 1\nworker = \"/x\"\nwrkr = \"/x\"\n",
        );
        let err = Config::load(&path).expect_err("typo must fail");
        assert!(err.to_string().contains("wrkr"), "error: {err}");
    }
}

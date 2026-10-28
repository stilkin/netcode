//! Spawning game-worker child processes and supervising them.

use std::path::PathBuf;

use tracing::info;

use crate::backplane_service::ServiceHandle;
use crate::registry::{SessionId, SessionSpec};

/// What the hub needs to start one worker process.
#[derive(Debug, Clone)]
pub struct SpawnRequest {
    /// Worker binary, e.g. `/opt/games/bombs-worker`.
    pub program: PathBuf,
    pub spec: SessionSpec,
    /// Session configuration handed to the worker as JSON (`--session-config`).
    pub session_config_json: String,
    /// Extra worker-specific arguments (test hooks, game options).
    pub extra_args: Vec<String>,
}

/// Something went wrong starting the child process.
#[derive(Debug, thiserror::Error)]
pub enum SpawnError {
    #[error("failed to spawn worker: {0}")]
    Io(#[from] std::io::Error),
    #[error("live-session limit reached")]
    SessionLimitReached,
}

impl ServiceHandle {
    /// Creates a pending session, spawns the worker, and supervises it: when
    /// the child exits for any reason the session is closed and its clients
    /// are detached. The hub never restarts workers.
    pub async fn spawn_session(&self, req: SpawnRequest) -> Result<SessionId, SpawnError> {
        let session_id = self
            .create_pending(req.spec.clone())
            .await
            .ok_or(SpawnError::SessionLimitReached)?;
        let mut command = tokio::process::Command::new(&req.program);
        command
            .arg("--backplane")
            .arg(self.local_addr().to_string())
            .arg("--token")
            .arg(self.token())
            .arg("--session-config")
            .arg(&req.session_config_json)
            .arg("--session-id")
            .arg(session_id.to_string())
            .args(&req.extra_args)
            .kill_on_drop(false);

        match command.spawn() {
            Ok(mut child) => {
                info!(session = session_id, program = %req.program.display(), pid = child.id(), "worker spawned");
                let service = self.clone();
                let session = session_id;
                tokio::spawn(async move {
                    let outcome = child.wait().await;
                    info!(session = session, ?outcome, "worker child exited");
                    service.worker_lost(session);
                });
                Ok(session_id)
            }
            Err(err) => {
                // Release the pending session; nobody will claim it.
                self.worker_lost(session_id);
                Err(SpawnError::Io(err))
            }
        }
    }
}

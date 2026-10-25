//! Shutdown signal handling: Ctrl-C (SIGINT) and, on Unix, SIGTERM.

/// Listens for shutdown signals. Handlers are installed on construction, so
/// create this *before* announcing readiness — a signal that races the boot
/// must still produce a clean stop.
pub struct Signals {
    #[cfg(unix)]
    term: Option<tokio::signal::unix::Signal>,
}

impl Signals {
    /// Installs the signal handlers.
    pub fn install() -> Self {
        #[cfg(unix)]
        {
            let term =
                match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                    Ok(term) => Some(term),
                    Err(err) => {
                        tracing::warn!(%err, "cannot listen for SIGTERM; Ctrl-C only");
                        None
                    }
                };
            Self { term }
        }
        #[cfg(not(unix))]
        Self {}
    }

    /// Resolves on the first shutdown signal.
    pub async fn first(&mut self) {
        #[cfg(unix)]
        {
            let ctrl_c = tokio::signal::ctrl_c();
            if let Some(term) = self.term.as_mut() {
                tokio::select! {
                    _ = ctrl_c => {}
                    _ = term.recv() => {}
                }
                return;
            }
            let _ = ctrl_c.await;
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}

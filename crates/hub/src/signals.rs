//! Shutdown signal handling: Ctrl-C (SIGINT) and, on Unix, SIGTERM.

/// Listens for shutdown signals. Handlers are installed on construction, so
/// create this *before* announcing readiness — a signal that races the boot
/// must still produce a clean stop.
pub struct Signals {
    #[cfg(unix)]
    interrupt: tokio::signal::unix::Signal,
    #[cfg(unix)]
    term: Option<tokio::signal::unix::Signal>,
}

impl Signals {
    /// Installs the signal handlers.
    ///
    /// Both handlers are armed synchronously here; nothing is registered
    /// lazily on first poll.
    pub fn install() -> Self {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            let interrupt =
                signal(SignalKind::interrupt()).expect("SIGINT handler registration cannot fail");
            let term = match signal(SignalKind::terminate()) {
                Ok(term) => Some(term),
                Err(err) => {
                    tracing::warn!(%err, "cannot listen for SIGTERM; Ctrl-C only");
                    None
                }
            };
            Self { interrupt, term }
        }
        #[cfg(not(unix))]
        Self {}
    }

    /// Resolves on the first shutdown signal.
    pub async fn first(&mut self) {
        #[cfg(unix)]
        {
            if let Some(term) = self.term.as_mut() {
                tokio::select! {
                    _ = self.interrupt.recv() => {}
                    _ = term.recv() => {}
                }
                return;
            }
            self.interrupt.recv().await;
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}

//! In-memory session registry: identity, metadata, capacity, lifecycle.
//!
//! Pure bookkeeping — no I/O, no locks. The hub core task owns it and exposes
//! results to the rest of the system through channels (see
//! `backplane_service`).

use std::collections::HashMap;

use thiserror::Error;

pub type SessionId = u32;
pub type ClientId = u32;

/// Id 0 means "no claim" in registration frames; it is never assigned.
const FIRST_ID: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Spawned, awaiting its worker's `Register` claim.
    Pending,
    /// Worker registered; accepting players.
    Active,
    /// Worker gone (or hub shutting down). Ids are never reused.
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: SessionId,
    pub state: SessionState,
    pub game_type: String,
    pub display_name: String,
    pub max_players: u16,
    pub payload_schema_version: u16,
    pub player_count: u16,
}

/// Session registration details shared by both registration modes.
#[derive(Debug, Clone)]
pub struct SessionSpec {
    pub game_type: String,
    pub display_name: String,
    pub max_players: u16,
    pub payload_schema_version: u16,
}

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum AttachError {
    #[error("session {0} not found")]
    NotFound(SessionId),
    #[error("session {0} is not accepting players")]
    NotJoinable(SessionId),
    #[error("session {0} is full")]
    Full(SessionId),
}

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum RegistryError {
    #[error("session {0} not found")]
    NotFound(SessionId),
    #[error("session {0} cannot transition to active from its current state")]
    NotPending(SessionId),
    #[error("client {1} is not attached to session {0}")]
    NoSuchClient(SessionId, ClientId),
}

struct SessionEntry {
    info: SessionInfo,
    client_ids: Vec<ClientId>,
}

/// The registry. Ids are monotonic and never reused while the process runs.
#[derive(Default)]
pub struct Registry {
    next_session_id: u32,
    next_client_id: u32,
    sessions: HashMap<SessionId, SessionEntry>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            next_session_id: FIRST_ID,
            next_client_id: FIRST_ID,
            sessions: HashMap::new(),
        }
    }

    fn alloc_session_id(&mut self) -> SessionId {
        let id = self.next_session_id;
        self.next_session_id = id
            .checked_add(1)
            .expect("hub would have to run for 136 years at 1 session/ns to exhaust ids");
        id
    }

    fn alloc_client_id(&mut self) -> ClientId {
        let id = self.next_client_id;
        self.next_client_id = id
            .checked_add(1)
            .expect("client ids are u32; exhausted");
        id
    }

    /// Creates a pending session for a worker the hub is about to spawn.
    pub fn create_pending(&mut self, spec: &SessionSpec) -> SessionId {
        let id = self.alloc_session_id();
        self.sessions.insert(
            id,
            SessionEntry {
                info: SessionInfo {
                    id,
                    state: SessionState::Pending,
                    game_type: spec.game_type.clone(),
                    display_name: spec.display_name.clone(),
                    max_players: spec.max_players,
                    payload_schema_version: spec.payload_schema_version,
                    player_count: 0,
                },
                client_ids: Vec::new(),
            },
        );
        id
    }

    /// Registers a self-managed worker's session as immediately active.
    pub fn register_self(&mut self, spec: &SessionSpec) -> SessionId {
        let id = self.create_pending(spec);
        self.activate(id).expect("fresh session is pending");
        id
    }

    /// Transitions a pending session to active (its worker registered).
    pub fn activate(&mut self, id: SessionId) -> Result<(), RegistryError> {
        let entry = self
            .sessions
            .get_mut(&id)
            .ok_or(RegistryError::NotFound(id))?;
        if entry.info.state != SessionState::Pending {
            return Err(RegistryError::NotPending(id));
        }
        entry.info.state = SessionState::Active;
        Ok(())
    }

    /// Attaches a client to an active session, enforcing capacity.
    pub fn attach(&mut self, id: SessionId, player_name: &str) -> Result<ClientId, AttachError> {
        let entry = self.sessions.get_mut(&id).ok_or(AttachError::NotFound(id))?;
        if entry.info.state != SessionState::Active {
            return Err(AttachError::NotJoinable(id));
        }
        if entry.info.player_count >= entry.info.max_players {
            return Err(AttachError::Full(id));
        }
        let client_id = self.alloc_client_id();
        entry.info.player_count += 1;
        entry.client_ids.push(client_id);
        let _ = player_name; // reserved for directory listings
        Ok(client_id)
    }

    /// Detaches a client from a session.
    pub fn detach(&mut self, id: SessionId, client_id: ClientId) -> Result<(), RegistryError> {
        let entry = self
            .sessions
            .get_mut(&id)
            .ok_or(RegistryError::NotFound(id))?;
        let pos = entry
            .client_ids
            .iter()
            .position(|&c| c == client_id)
            .ok_or(RegistryError::NoSuchClient(id, client_id))?;
        entry.client_ids.swap_remove(pos);
        entry.info.player_count = entry.client_ids.len() as u16;
        Ok(())
    }

    /// Closes a session in any state; returns the ids of clients that were
    /// attached (and must now be dropped by the caller). Idempotent: closing
    /// a closed session returns `None`.
    pub fn close(&mut self, id: SessionId) -> Option<Vec<ClientId>> {
        let entry = self.sessions.get_mut(&id)?;
        if entry.info.state == SessionState::Closed {
            return None;
        }
        entry.info.state = SessionState::Closed;
        entry.info.player_count = 0;
        Some(std::mem::take(&mut entry.client_ids))
    }

    /// Metadata for one session in any state.
    pub fn get(&self, id: SessionId) -> Option<SessionInfo> {
        self.sessions.get(&id).map(|entry| entry.info.clone())
    }

    /// All sessions that are not closed — what a server browser shows.
    pub fn list(&self) -> Vec<SessionInfo> {
        let mut sessions: Vec<SessionInfo> = self
            .sessions
            .values()
            .map(|entry| entry.info.clone())
            .filter(|info| info.state != SessionState::Closed)
            .collect();
        sessions.sort_by_key(|info| info.id);
        sessions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> SessionSpec {
        SessionSpec {
            game_type: "echo-arena".into(),
            display_name: "Echo Arena".into(),
            max_players: 2,
            payload_schema_version: 1,
        }
    }

    /// (session-registry: Unique session ids)
    #[test]
    fn concurrent_registrations_get_distinct_ids() {
        let mut registry = Registry::new();
        let a = registry.create_pending(&spec());
        let b = registry.register_self(&spec());
        assert_ne!(a, b);
    }

    /// (session-registry: Unique session ids)
    #[test]
    fn ids_are_not_reused_after_close() {
        let mut registry = Registry::new();
        let first = registry.create_pending(&spec());
        registry.close(first);
        let second = registry.create_pending(&spec());
        assert!(second > first);
    }

    /// (session-registry: Session metadata)
    #[test]
    fn player_count_tracks_membership() {
        let mut registry = Registry::new();
        let id = registry.register_self(&spec());
        let c1 = registry.attach(id, "p1").expect("attach 1");
        let c2 = registry.attach(id, "p2").expect("attach 2");
        assert_eq!(registry.get(id).expect("session").player_count, 2);
        registry.detach(id, c1).expect("detach");
        registry.detach(id, c2).expect("detach");
        assert_eq!(registry.get(id).expect("session").player_count, 0);
    }

    /// (session-registry: Capacity enforcement)
    #[test]
    fn full_session_refuses_attach() {
        let mut registry = Registry::new();
        let id = registry.register_self(&spec());
        registry.attach(id, "p1").expect("attach 1");
        registry.attach(id, "p2").expect("attach 2");
        assert_eq!(registry.attach(id, "p3"), Err(AttachError::Full(id)));
        assert_eq!(registry.get(id).expect("session").player_count, 2);
    }

    /// (session-registry: Session lifecycle)
    #[test]
    fn pending_sessions_are_not_joinable() {
        let mut registry = Registry::new();
        let id = registry.create_pending(&spec());
        assert_eq!(registry.attach(id, "p1"), Err(AttachError::NotJoinable(id)));
        registry.activate(id).expect("activate");
        registry.attach(id, "p1").expect("joinable after activation");
    }

    /// (session-registry: Session lifecycle)
    #[test]
    fn close_is_idempotent_and_reports_clients() {
        let mut registry = Registry::new();
        let id = registry.register_self(&spec());
        registry.attach(id, "p1").expect("attach");
        let detached = registry.close(id).expect("first close");
        assert_eq!(detached.len(), 1);
        assert_eq!(registry.close(id), None, "second close is a no-op");
        assert_eq!(registry.attach(id, "p2"), Err(AttachError::NotJoinable(id)));
    }

    /// (session-registry: Unique session ids)
    #[test]
    fn activating_a_non_pending_session_fails() {
        let mut registry = Registry::new();
        let id = registry.register_self(&spec());
        assert_eq!(registry.activate(id), Err(RegistryError::NotPending(id)));
    }
}

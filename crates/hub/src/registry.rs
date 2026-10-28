//! In-memory session registry: identity, metadata, capacity, lifecycle.
//!
//! Pure bookkeeping — no I/O, no locks. The hub core task owns it and exposes
//! results to the rest of the system through channels (see
//! `backplane_service`).

use std::collections::{HashMap, VecDeque};

use thiserror::Error;

pub type SessionId = u32;
pub type ClientId = u32;

/// Id 0 means "no claim" in registration frames; it is never assigned.
const FIRST_ID: u32 = 1;

/// Number of recently closed sessions whose metadata remains available.
const CLOSED_HISTORY_LIMIT: usize = 1024;

/// Default combined pending and active session limit.
pub const DEFAULT_MAX_SESSIONS: usize = 64;

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
pub struct Registry {
    next_session_id: u32,
    next_client_id: u32,
    max_sessions: usize,
    sessions: HashMap<SessionId, SessionEntry>,
    closed_order: VecDeque<SessionId>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    pub fn new() -> Self {
        Self::with_session_limit(DEFAULT_MAX_SESSIONS)
    }

    pub fn with_session_limit(max_sessions: usize) -> Self {
        Self {
            next_session_id: FIRST_ID,
            next_client_id: FIRST_ID,
            max_sessions,
            sessions: HashMap::new(),
            closed_order: VecDeque::new(),
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
        self.next_client_id = id.checked_add(1).expect("client ids are u32; exhausted");
        id
    }

    /// Creates a pending session for a worker the hub is about to spawn.
    /// Returns `None` without consuming an id when the live-session limit
    /// is full. Pending and active sessions count; closed tombstones do not.
    pub fn create_pending(&mut self, spec: &SessionSpec) -> Option<SessionId> {
        let live = self
            .sessions
            .values()
            .filter(|entry| entry.info.state != SessionState::Closed)
            .count();
        if live >= self.max_sessions {
            return None;
        }
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
        Some(id)
    }

    /// Registers a self-managed worker's session as immediately active.
    pub fn register_self(&mut self, spec: &SessionSpec) -> Option<SessionId> {
        let id = self.create_pending(spec)?;
        self.activate(id).expect("fresh session is pending");
        Some(id)
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
        // Validate before allocating, so a failed attach consumes no id.
        {
            let entry = self.sessions.get(&id).ok_or(AttachError::NotFound(id))?;
            if entry.info.state != SessionState::Active {
                return Err(AttachError::NotJoinable(id));
            }
            if entry.info.player_count >= entry.info.max_players {
                return Err(AttachError::Full(id));
            }
        }
        let client_id = self.alloc_client_id();
        let entry = self.sessions.get_mut(&id).expect("session checked above");
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
    /// a closed session returns `None`. The oldest tombstone is evicted when
    /// the closed history exceeds its cap.
    pub fn close(&mut self, id: SessionId) -> Option<Vec<ClientId>> {
        let entry = self.sessions.get_mut(&id)?;
        if entry.info.state == SessionState::Closed {
            return None;
        }
        entry.info.state = SessionState::Closed;
        entry.info.player_count = 0;
        let clients = std::mem::take(&mut entry.client_ids);

        self.closed_order.push_back(id);
        while self.closed_order.len() > CLOSED_HISTORY_LIMIT {
            let evicted = self
                .closed_order
                .pop_front()
                .expect("overflow has an oldest");
            self.sessions.remove(&evicted);
        }
        Some(clients)
    }

    /// Ids of the clients currently attached to an active session, in
    /// attach order. `None` for unknown or non-active sessions.
    pub fn client_ids(&self, id: SessionId) -> Option<&[ClientId]> {
        self.sessions
            .get(&id)
            .filter(|entry| entry.info.state == SessionState::Active)
            .map(|entry| entry.client_ids.as_slice())
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
        let a = registry.create_pending(&spec()).expect("create a");
        let b = registry.register_self(&spec()).expect("register b");
        assert_ne!(a, b);
    }

    /// (session-registry: Unique session ids)
    #[test]
    fn ids_are_not_reused_after_close() {
        let mut registry = Registry::new();
        let first = registry.create_pending(&spec()).expect("first");
        registry.close(first);
        let second = registry.create_pending(&spec()).expect("second");
        assert!(second > first);
    }

    /// (session-registry: Session metadata)
    #[test]
    fn player_count_tracks_membership() {
        let mut registry = Registry::new();
        let id = registry.register_self(&spec()).expect("register");
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
        let id = registry.register_self(&spec()).expect("register");
        registry.attach(id, "p1").expect("attach 1");
        registry.attach(id, "p2").expect("attach 2");
        assert_eq!(registry.attach(id, "p3"), Err(AttachError::Full(id)));
        assert_eq!(registry.get(id).expect("session").player_count, 2);
    }

    /// (session-registry: Session lifecycle)
    #[test]
    fn pending_sessions_are_not_joinable() {
        let mut registry = Registry::new();
        let id = registry.create_pending(&spec()).expect("create pending");
        assert_eq!(registry.attach(id, "p1"), Err(AttachError::NotJoinable(id)));
        registry.activate(id).expect("activate");
        registry
            .attach(id, "p1")
            .expect("joinable after activation");
    }

    /// (session-registry: Session lifecycle)
    #[test]
    fn close_is_idempotent_and_reports_clients() {
        let mut registry = Registry::new();
        let id = registry.register_self(&spec()).expect("register");
        registry.attach(id, "p1").expect("attach");
        let detached = registry.close(id).expect("first close");
        assert_eq!(detached.len(), 1);
        assert_eq!(registry.close(id), None, "second close is a no-op");
        assert_eq!(registry.attach(id, "p2"), Err(AttachError::NotJoinable(id)));
    }

    /// (session-registry: Live session capacity) Pending and active entries
    /// fill the limit; rejection consumes no id and close frees a slot.
    #[test]
    fn live_session_limit_is_atomic_and_close_frees_capacity() {
        let mut registry = Registry::with_session_limit(1);
        let first = registry.create_pending(&spec()).expect("first pending");
        assert_eq!(registry.create_pending(&spec()), None);
        assert_eq!(registry.list().len(), 1);

        registry.activate(first).expect("activate first");
        assert_eq!(registry.register_self(&spec()), None);
        registry.close(first).expect("close first");

        let second = registry.register_self(&spec()).expect("capacity released");
        assert_eq!(second, first + 1, "rejected attempts consumed no id");
        assert_eq!(
            registry.get(first).expect("closed tombstone remains").state,
            SessionState::Closed
        );
    }

    /// (session-registry: Live session capacity) Zero denies creation rather
    /// than acting as an unlimited sentinel.
    #[test]
    fn zero_session_limit_denies_creation() {
        let mut registry = Registry::with_session_limit(0);
        assert_eq!(registry.create_pending(&spec()), None);
        assert_eq!(registry.register_self(&spec()), None);
        assert!(registry.list().is_empty());
    }

    /// (session-registry: Closed session history) Overflow evicts only the
    /// oldest tombstone; live sessions and newer tombstones remain.
    #[test]
    fn closed_history_is_bounded_and_evicts_oldest() {
        let mut registry = Registry::new();
        let open = registry.create_pending(&spec()).expect("open session");
        let mut closed = Vec::new();
        for _ in 0..=CLOSED_HISTORY_LIMIT {
            let id = registry.create_pending(&spec()).expect("close candidate");
            registry.close(id);
            closed.push(id);
        }

        let oldest = closed[0];
        let newest = *closed.last().expect("newest closed id");
        assert!(registry.get(oldest).is_none(), "oldest was evicted");
        assert_eq!(
            registry.attach(oldest, "p"),
            Err(AttachError::NotFound(oldest))
        );
        assert_eq!(
            registry.get(newest).expect("newest tombstone").state,
            SessionState::Closed
        );
        assert_eq!(
            registry.attach(newest, "p"),
            Err(AttachError::NotJoinable(newest))
        );
        assert_eq!(
            registry.get(open).expect("open session remains").state,
            SessionState::Pending
        );
        assert_eq!(
            registry.sessions.len(),
            CLOSED_HISTORY_LIMIT + 1,
            "closed entries plus the open session"
        );

        let next = registry.create_pending(&spec()).expect("next id");
        assert!(next > newest, "evicted ids are never reused");
    }

    /// (session-registry: Closed session history) An idempotent close does
    /// not enqueue another copy and eventually evict the tombstone itself.
    #[test]
    fn repeated_close_does_not_consume_history() {
        let mut registry = Registry::new();
        let id = registry.register_self(&spec()).expect("register");
        registry.close(id).expect("first close");
        for _ in 0..=CLOSED_HISTORY_LIMIT {
            assert_eq!(registry.close(id), None);
        }
        assert_eq!(
            registry.get(id).expect("tombstone remains").state,
            SessionState::Closed
        );
        assert_eq!(registry.attach(id, "p"), Err(AttachError::NotJoinable(id)));
    }

    /// (session-registry: Unique session ids)
    #[test]
    fn activating_a_non_pending_session_fails() {
        let mut registry = Registry::new();
        let id = registry.register_self(&spec()).expect("register");
        assert_eq!(registry.activate(id), Err(RegistryError::NotPending(id)));
    }
}

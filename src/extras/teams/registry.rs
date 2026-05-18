//! `TeamRegistry` — lifecycle management for named teams.
//!
//! Mirrors `SubagentRegistry` in structure: a `Mutex`-protected `HashMap`
//! mapping team names to `Arc<RwLock<Team>>`. Clones cheaply — all clones
//! share the same underlying state.
//!
//! # Lock discipline
//!
//! The outer `Mutex` is held only for brief synchronous operations (insert,
//! remove, clone Arc). It is **always released before** any `RwLock<Team>`
//! guard is acquired. `get()` returns an `Arc<RwLock<Team>>` — the caller
//! receives the Arc after the outer Mutex is dropped, making it structurally
//! impossible to hold both locks simultaneously.
//!
//! # Split-brain note
//!
//! After `remove()`, any caller holding a previously cloned `Arc<RwLock<Team>>`
//! still has a valid strong reference to the removed team. This is intentional
//! and matches the `SubagentRegistry::stop()` convention — callers that hold
//! stale arcs operate on orphaned state. Name reuse after removal is an error
//! (returned by `create()`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use crate::extras::subagent::SubagentRegistry;

use super::team::Team;

struct TeamRegistryInner {
    teams: HashMap<String, Arc<RwLock<Team>>>,
}

impl TeamRegistryInner {
    fn new() -> Self {
        TeamRegistryInner {
            teams: HashMap::new(),
        }
    }
}

/// Thread-safe registry of named teams.
///
/// Always held behind `Arc<TeamRegistry>` — use `Arc::clone` to share.
///
/// Holds a shared reference to the `SubagentRegistry` so that team operations
/// (messaging, membership) have access to subagent lifecycle without callers
/// needing to pass the registry at every call site.
///
/// The inner map uses `Arc<RwLock<Team>>` per entry so callers can hold a
/// reference to a specific team beyond the lifetime of the outer `Mutex` guard
/// — this makes it structurally impossible to hold both locks simultaneously.
pub struct TeamRegistry {
    inner: Mutex<TeamRegistryInner>,
    agents: Arc<SubagentRegistry>,
}

impl TeamRegistry {
    /// Create a new empty registry backed by the given subagent registry.
    pub fn new(agents: Arc<SubagentRegistry>) -> Self {
        TeamRegistry {
            inner: Mutex::new(TeamRegistryInner::new()),
            agents,
        }
    }

    /// Return a reference to the shared subagent registry.
    ///
    /// Used by callers (e.g. tool builders) that need access to the registry
    /// without it being exposed directly on `TeamContext`.
    pub fn agents(&self) -> &Arc<SubagentRegistry> {
        &self.agents
    }

    /// Create a new named team, backed by the registry's `SubagentRegistry`.
    ///
    /// Returns `Err` if a team with that name already exists.
    pub fn create(&self, name: String) -> anyhow::Result<Arc<RwLock<Team>>> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.teams.contains_key(&name) {
            anyhow::bail!("team '{}' already exists", name);
        }
        let team = Arc::new(RwLock::new(Team::new(Arc::clone(&self.agents))));
        inner.teams.insert(name, Arc::clone(&team));
        Ok(team)
    }

    /// Look up a team by name, returning a cloned Arc.
    ///
    /// The outer Mutex is released before the Arc is returned — the caller
    /// acquires the inner `RwLock<Team>` guard independently.
    pub fn get(&self, name: &str) -> Option<Arc<RwLock<Team>>> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.teams.get(name).cloned()
    }

    /// Remove a team by name. Returns `Ok(())` if found, `Err` if not.
    ///
    /// Any cloned `Arc<RwLock<Team>>` held by callers remains valid but
    /// orphaned — see module-level split-brain note.
    pub fn remove(&self, name: &str) -> anyhow::Result<()> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.teams.remove(name).is_some() {
            Ok(())
        } else {
            anyhow::bail!("team '{}' not found", name)
        }
    }

    /// List all team names.
    pub fn list(&self) -> Vec<String> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.teams.keys().cloned().collect()
    }
}

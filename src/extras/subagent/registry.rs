//! `SubagentRegistry` — lifecycle management for background agents.
//!
//! # Synchronization model
//!
//! The registry uses `std::sync::Mutex` for the outer map. All registry methods
//! are synchronous within the lock guard — the lock is never held across `.await`.
//! Pattern for async operations:
//!
//! ```rust
//! // 1. Acquire lock, extract what's needed, drop guard
//! let info = {
//!     let guard = self.inner.lock().unwrap();
//!     guard.handles.get(&id).map(|h| Arc::clone(&h.state))
//! }; // guard dropped here
//!
//! // 2. Async work outside the lock
//! some_async_op(info).await;
//!
//! // 3. Re-acquire lock to write result
//! {
//!     let mut guard = self.inner.lock().unwrap();
//!     guard.handles.get_mut(&id).unwrap().state.write().unwrap().status = ...;
//! }
//! ```

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use rig::completion::Message;
use tokio::sync::mpsc;

use crate::provider::build_agent_from_model;
use crate::cli::Cli;
use crate::config::Config;
use crate::context::ContextFiles;
#[cfg(feature = "mcp")]
use crate::extras::mcp::McpClientManager;
use crate::permission::ask;
use crate::permission::checker::PermCheck;
use crate::provider::AnyClient;
use crate::sandbox::Sandbox;
use crate::session::Session;

use super::bus::{BusSender, spawn_agent_relay, spawn_perm_relay};
use super::handle::{SubagentHandle, SubagentInner};
use super::{ContextMode, SubagentId, SubagentSnapshot, SubagentStatus};
use crate::agent::toolset::ToolSet;
use crate::provider::AnyAgent;

/// Configuration for spawning a new subagent.
pub struct SpawnConfig {
    pub name: String,
    pub prompt: String,
    pub context_mode: ContextMode,
    pub tool_set: ToolSet,
    /// Model name to use (e.g. from session.model). If empty, uses CLI/config default.
    pub model_name: String,
}

struct RegistryInner {
    handles: HashMap<SubagentId, SubagentHandle>,
    by_name: HashMap<String, SubagentId>,
    next_id: u32,
}

impl RegistryInner {
    fn new() -> Self {
        RegistryInner {
            handles: HashMap::new(),
            by_name: HashMap::new(),
            next_id: 1,
        }
    }

    fn next_id(&mut self) -> SubagentId {
        let id = SubagentId(self.next_id);
        self.next_id += 1;
        id
    }
}

/// Thread-safe registry of all background subagents.
///
/// Always held behind `Arc<SubagentRegistry>` — use `Arc::clone` to share.
pub struct SubagentRegistry {
    inner: Mutex<RegistryInner>,
}

impl SubagentRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        SubagentRegistry {
            inner: Mutex::new(RegistryInner::new()),
        }
    }

    /// Returns `true` if the registry contains a live entry for `id`.
    ///
    /// Used by the UI bus event handler to discard stale events from stopped
    /// subagents (which can arrive after `stop()` due to MPSC channel buffering).
    pub fn contains_id(&self, id: SubagentId) -> bool {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.handles.contains_key(&id)
    }

    /// Look up a subagent by name, returning its id.
    pub fn id_by_name(&self, name: &str) -> Option<SubagentId> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.by_name.get(name).copied()
    }

    /// Returns a clone of the shared inner state for the given id.
    /// Used by relay tasks and the UI loop to read status without holding
    /// the registry Mutex.
    pub fn state_arc(&self, id: SubagentId) -> Option<Arc<RwLock<SubagentInner>>> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.handles.get(&id).map(|h| Arc::clone(&h.state))
    }

    /// Spawn a new subagent with the given configuration.
    ///
    /// # Async safety
    ///
    /// The registry Mutex is held only for brief synchronous sections.
    /// `build_agent_inner` (async) runs outside the lock.
    #[allow(clippy::too_many_arguments)] // spawning a subagent requires all of these parameters
    pub async fn spawn(
        &self,
        config: SpawnConfig,
        client: &AnyClient,
        cli: &Cli,
        cfg: &Config,
        context: &ContextFiles,
        lead_session: Option<&Session>,
        permission: Option<PermCheck>,
        sandbox: Sandbox,
        bus_tx: BusSender,
        #[cfg(feature = "mcp")] mcp_manager: Option<&McpClientManager>,
    ) -> anyhow::Result<SubagentId> {
        // Phase 1: Validate and allocate ID (synchronous, brief lock).
        let id = {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if guard.by_name.contains_key(&config.name) {
                anyhow::bail!("subagent '{}' already exists", config.name);
            }
            guard.next_id()
        };

        // Phase 2: Build permission channel for this subagent (no lock needed).
        let (ask_tx, ask_rx): (ask::AskSender, ask::AskReceiver) = mpsc::channel(32);

        // Phase 3: Build the agent (async, outside lock).
        let model = client.completion_model(config.model_name.clone());
        let agent = build_agent_from_model(
            model,
            cli,
            cfg,
            context,
            permission.clone(),
            Some(ask_tx.clone()),
            sandbox.clone(),
            &config.tool_set,
            #[cfg(feature = "mcp")]
            mcp_manager,
        )
        .await;

        // Phase 4: Build initial history from context mode (no lock).
        let initial_history: Vec<Message> = match config.context_mode {
            ContextMode::Fresh => Vec::new(),
            ContextMode::Fork => {
                if let Some(session) = lead_session {
                    crate::agent::runner::convert_history(session)
                } else {
                    Vec::new()
                }
            }
        };

        // Phase 5: Spawn the agent runner (no lock).
        // Clone before calling spawn_runner — the agent is stored in the handle for
        // inbox-drain re-spawns (task 3.5). spawn_runner consumes self, so we clone first.
        let runner = agent
            .clone()
            .spawn_runner(config.prompt.clone(), initial_history);

        // Phase 6: Spawn relay tasks (no lock).
        let state = Arc::new(RwLock::new(SubagentInner::new()));
        {
            let mut inner = state.write().unwrap();
            inner.status = SubagentStatus::Running;
        }

        let relay_handle = spawn_agent_relay(id, runner.event_rx, bus_tx.clone());
        let perm_relay_handle = spawn_perm_relay(id, ask_rx, bus_tx);

        // Phase 7: Insert handle into registry (synchronous, brief lock).
        let handle = SubagentHandle::new(
            id,
            config.name.clone(),
            Arc::clone(&state),
            agent,
            relay_handle,
            perm_relay_handle,
        );
        {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            guard.handles.insert(id, handle);
            guard.by_name.insert(config.name, id);
        }

        Ok(id)
    }

    /// Send a message to a subagent's inbox.
    ///
    /// If the subagent is idle (Done or Idle), the caller is responsible for
    /// re-spawning the runner (done in the UI bus event handler).
    pub fn send_message(&self, id: SubagentId, message: String) -> anyhow::Result<()> {
        let state_arc = self
            .state_arc(id)
            .ok_or_else(|| anyhow::anyhow!("subagent {} not found", id))?;
        let mut inner = state_arc.write().unwrap();
        inner.inbox.push_back(message);
        Ok(())
    }

    /// Drain one message from the inbox (called after a runner completes).
    pub fn pop_inbox_message(&self, id: SubagentId) -> Option<String> {
        let state_arc = self.state_arc(id)?;
        let mut inner = state_arc.write().unwrap();
        inner.inbox.pop_front()
    }

    /// Stop a subagent: abort relay tasks, mark Done, keep name→id mapping for history.
    pub fn stop(&self, id: SubagentId) -> anyhow::Result<()> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(handle) = guard.handles.get(&id) {
            handle.abort();
        }
        guard.handles.remove(&id);
        // Remove name→id mapping too, allowing reuse of the name.
        guard.by_name.retain(|_, v| *v != id);
        Ok(())
    }

    /// Returns a snapshot of all agents for status display.
    pub fn list(&self) -> Vec<SubagentSnapshot> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .handles
            .values()
            .map(|h| {
                let inner = h.state.read().unwrap();
                SubagentSnapshot {
                    id: h.id,
                    name: h.name.clone(),
                    status: inner.status.clone(),
                    final_response: inner.final_response.clone(),
                }
            })
            .collect()
    }

    /// Update status for a subagent (called from UI bus event handler).
    pub fn set_status(&self, id: SubagentId, status: SubagentStatus) {
        if let Some(state_arc) = self.state_arc(id)
            && let Ok(mut inner) = state_arc.write()
        {
            inner.status = status;
        }
    }

    /// Set final response for a subagent (called on BusEvent::Done).
    pub fn set_final_response(&self, id: SubagentId, response: String) {
        if let Some(state_arc) = self.state_arc(id)
            && let Ok(mut inner) = state_arc.write()
        {
            inner.final_response = Some(response);
        }
    }

    /// Get the name for a subagent id (for display in UI).
    pub fn name(&self, id: SubagentId) -> Option<String> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.handles.get(&id).map(|h| h.name.clone())
    }

    /// Clone the stored agent for a subagent. Used by the inbox-drain re-spawn path.
    ///
    /// Returns `None` if the subagent is not found.
    pub fn get_agent(&self, id: SubagentId) -> Option<AnyAgent> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.handles.get(&id).map(|h| h.agent.clone())
    }

    /// Replace the relay task handles for a subagent, aborting the old ones first.
    ///
    /// This is the inbox-drain re-spawn path (task 3.5). Abort-first ordering
    /// closes the race window where old and new relay tasks are simultaneously
    /// alive for the same SubagentId.
    pub fn replace_relay_handles(
        &self,
        id: SubagentId,
        new_relay: tokio::task::JoinHandle<()>,
        new_perm_relay: tokio::task::JoinHandle<()>,
    ) -> anyhow::Result<()> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let handle = guard
            .handles
            .get_mut(&id)
            .ok_or_else(|| anyhow::anyhow!("subagent {} not found", id))?;
        // Abort-first: old relays are cancelled before new handles are stored.
        handle.replace_handles(new_relay, new_perm_relay);
        Ok(())
    }
}

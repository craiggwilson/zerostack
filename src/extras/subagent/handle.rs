//! `SubagentHandle` — per-subagent lifecycle and shared state.
//!
//! # Synchronization model
//!
//! `SubagentHandle` is stored inside `SubagentRegistry`'s `std::sync::Mutex`.
//! It does NOT contain `view_buffer` — that lives in the UI event loop state
//! as a separate `HashMap<SubagentId, Vec<LineEntry>>`. This ensures:
//!
//! - No lock is ever needed for view buffer writes (UI loop is sole writer).
//! - The registry Mutex is held only briefly (get handle info, insert, remove).
//! - No `.await` is ever called while the registry Mutex is held.
//!
//! `SubagentInner` (the fields shared with relay tasks) is behind `Arc<RwLock<>>`.
//! It is read-heavy (status listing) with occasional writes (inbox push, completion).
//!
//! # Agent storage and ask_tx pairing
//!
//! `SubagentHandle` stores the `AnyAgent` clone built at spawn time. This agent
//! contains an internal `ask_tx` permanently bound to the `ask_rx` consumed by
//! the initial `spawn_perm_relay`. When the relay is replaced on inbox-drain
//! re-spawn (`replace_handles`), the new perm relay uses a fresh `ask_rx2` from
//! a new channel pair — but the stored agent still sends permission requests to
//! the original (now-dead) `ask_tx`. As a result, **re-spawned inbox-drain runs
//! have no permission ask support**: permission requests fail closed with
//! "Permission system unavailable" (see `src/agent/tools/mod.rs`). This is an
//! accepted MVP limitation. Do not attempt to rebind ask_tx — mpsc channel pairs
//! are permanent; a new sender cannot share the original receiver.

use std::collections::VecDeque;
use std::sync::{Arc, RwLock};

use tokio::task::JoinHandle;

use super::{SubagentId, SubagentStatus};
use crate::provider::AnyAgent;

/// Shared inner state of a subagent, accessible by both the registry and relay tasks.
///
/// Behind `Arc<RwLock<>>` — read-heavy (status polling) with occasional writes.
/// `view_buffer` is NOT here — see module doc.
pub struct SubagentInner {
    /// Current lifecycle status.
    pub status: SubagentStatus,
    /// Pending messages waiting to be injected into the next runner turn.
    pub inbox: VecDeque<String>,
    /// The final response text from the last completed runner turn.
    pub final_response: Option<String>,
}

impl SubagentInner {
    pub fn new() -> Self {
        SubagentInner {
            status: SubagentStatus::Idle,
            inbox: VecDeque::new(),
            final_response: None,
        }
    }
}

/// Per-subagent handle stored in the registry.
///
/// Contains the shared state Arc, relay task handles, and the agent clone
/// used for re-spawning runners after inbox drain.
/// Does NOT contain `view_buffer` — that is owned exclusively by the UI event loop.
pub struct SubagentHandle {
    pub id: SubagentId,
    pub name: String,
    /// Shared state: status, inbox, final response. Behind RwLock — read-heavy.
    pub state: Arc<RwLock<SubagentInner>>,
    /// Agent clone used to re-spawn runners on inbox drain (task 3.5).
    /// Note: this agent's internal ask_tx is permanently bound to the original
    /// ask_rx consumed at spawn time. After relay replacement, the perm relay
    /// uses a fresh channel pair — the agent's ask_tx points to a dead receiver.
    /// Re-spawned runs therefore have no permission ask support (fails closed).
    pub agent: AnyAgent,
    /// The relay task's JoinHandle. Aborted on `stop()` to prevent stale BusEvents.
    relay_handle: JoinHandle<()>,
    /// The permission relay task's JoinHandle. Also aborted on `stop()`.
    perm_relay_handle: JoinHandle<()>,
}

impl SubagentHandle {
    /// Create a new handle with the given relay task handles.
    pub fn new(
        id: SubagentId,
        name: String,
        state: Arc<RwLock<SubagentInner>>,
        agent: AnyAgent,
        relay_handle: JoinHandle<()>,
        perm_relay_handle: JoinHandle<()>,
    ) -> Self {
        SubagentHandle {
            id,
            name,
            state,
            agent,
            relay_handle,
            perm_relay_handle,
        }
    }

    /// Abort the relay tasks and mark the subagent as stopped.
    ///
    /// # Safety
    ///
    /// `abort()` is asynchronous — it schedules cancellation. The relay task
    /// may emit a small number of buffered events before it exits. The UI
    /// bus event handler must discard events for unknown SubagentIds to handle
    /// this window correctly.
    pub fn abort(&self) {
        self.relay_handle.abort();
        self.perm_relay_handle.abort();
        if let Ok(mut inner) = self.state.write() {
            inner.status = SubagentStatus::Done;
        }
    }

    /// Replace relay task handles, aborting old ones first.
    ///
    /// Called from the inbox-drain re-spawn path (task 3.5). Abort-first ordering
    /// closes the window where old and new relay tasks are both alive simultaneously.
    pub fn replace_handles(&mut self, new_relay: JoinHandle<()>, new_perm_relay: JoinHandle<()>) {
        self.relay_handle.abort();
        self.perm_relay_handle.abort();
        self.relay_handle = new_relay;
        self.perm_relay_handle = new_perm_relay;
    }
}

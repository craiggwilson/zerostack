//! Subagent subsystem: spawn, track, and communicate with background agents.
//!
//! Feature-gated by `#[cfg(feature = "subagent")]`. All public types in this
//! module are part of the subagent API surface.

pub mod bus;
pub mod handle;
pub mod registry;
pub mod tools;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
// re-exported for external consumers; not used in subagent-only build
pub use registry::SubagentRegistry;

/// Unique identifier for a subagent. Assigned monotonically by [`SubagentRegistry`].
/// Plain u32 — no UUID overhead, stable for the lifetime of the process.
///
/// [`SubagentRegistry`]: registry::SubagentRegistry
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubagentId(pub u32);

impl std::fmt::Display for SubagentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Lifecycle status of a subagent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubagentStatus {
    /// Runner is active and processing.
    Running,
    /// Runner is idle, waiting for messages.
    Idle,
    /// Subagent has completed all work.
    Done,
    /// Subagent stopped with an error.
    Error(String),
}

impl std::fmt::Display for SubagentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubagentStatus::Running => write!(f, "running"),
            SubagentStatus::Idle => write!(f, "idle"),
            SubagentStatus::Done => write!(f, "done"),
            SubagentStatus::Error(e) => write!(f, "error: {}", e),
        }
    }
}

/// How the subagent's initial session history is populated at spawn time.
#[derive(Debug, Clone)]
pub enum ContextMode {
    /// Start with an empty history — clean slate.
    Fresh,
    /// Fork: copy the lead's compacted history at spawn time.
    /// The subagent's history diverges from this point onward.
    Fork,
}

/// Routing target for messages sent between agents via `BusEvent::Message`.
///
/// Nothing currently emits `BusEvent::Message` from a relay — this is
/// infrastructure for future agent-to-agent communication. The handler
/// in `ui/mod.rs` is complete; the relay-side emission is not yet implemented.
#[derive(Debug, Clone)]
#[allow(dead_code)] // relay emission not yet implemented; handler in ui/mod.rs is complete
pub enum MessageTarget {
    /// Route to the lead agent's pending injections queue.
    Lead,
    /// Route to a specific subagent's inbox.
    Subagent(SubagentId),
    /// Deliver to all members of a group.
    Broadcast,
}

/// Snapshot of a subagent's state for display purposes (e.g., `/agent status`).
#[derive(Debug, Clone)]
pub struct SubagentSnapshot {
    pub id: SubagentId,
    pub name: String,
    pub status: SubagentStatus,
    pub final_response: Option<String>,
}

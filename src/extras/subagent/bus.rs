//! Bus event types and relay task for the subagent event multiplexer.
//!
//! The bus is a single `mpsc::channel<BusEvent>` that aggregates events from
//! all running subagents. Each subagent spawns a relay task that reads from
//! its `AgentRunner.event_rx` and re-emits tagged `BusEvent`s onto the shared bus.
//!
//! # Channel lifecycle
//!
//! The bus channel stays open as long as at least one `Sender<BusEvent>` clone
//! is alive. The UI loop holds a `_bus_tx_keep` clone for this purpose, so
//! the bus never closes while the loop is running — even when no subagents
//! are active. The `select!` arm uses `Some(ev) = bus_rx.recv()` so it stays
//! pending (not spinning) when the channel is empty or closed.
//!
//! # Relay task lifecycle
//!
//! Each relay task is started in `SubagentRegistry::spawn()` and its
//! `JoinHandle<()>` is stored on `SubagentHandle`. On `stop()`, the handle
//! is aborted before the runner is dropped, preventing stale events from
//! entering the bus after the registry entry is removed.
//!
//! # Orphaned events
//!
//! Because `abort()` is asynchronous (schedules cancellation), a small number
//! of events may still be in the bus channel when the registry entry is
//! removed. The UI bus event handler MUST guard against this:
//!
//! ```rust
//! if !registry.contains_id(bus_event.id) {
//!     continue; // silently discard stale event from a stopped subagent
//! }
//! ```

use compact_str::CompactString;
use tokio::sync::mpsc;

use crate::event::AgentEvent;
use crate::permission::ask::AskReceiver;

use super::SubagentId;

/// Bus capacity: handles bursts from many concurrent subagents.
pub const BUS_CAPACITY: usize = 1024;

/// Multiplexed event from any subagent, tagged with its source id.
#[derive(Debug)]
#[allow(dead_code)] // Message variant: relay emission not yet implemented; handler in ui/mod.rs is complete
pub enum BusEvent {
    /// A streaming token from a subagent's response.
    Token { id: SubagentId, text: CompactString },
    /// A tool call initiated by a subagent.
    ToolCall {
        id: SubagentId,
        name: CompactString,
        args: serde_json::Value,
    },
    /// A tool result received by a subagent.
    ToolResult {
        id: SubagentId,
        output: CompactString,
    },
    /// A subagent has completed its turn.
    Done {
        id: SubagentId,
        response: CompactString,
        tokens: u64,
        cost: f64,
    },

    /// A subagent encountered an error.
    Error {
        id: SubagentId,
        message: CompactString,
    },
    /// A subagent is requesting a permission decision from the user.
    PermAsk {
        id: SubagentId,
        request: crate::permission::ask::AskRequest,
    },
    /// A subagent is sending a message to another subagent, the lead, or all.
    Message {
        from_id: SubagentId,
        from_name: compact_str::CompactString,
        to: super::MessageTarget,
        content: compact_str::CompactString,
    },
}

impl BusEvent {
    /// Returns the source `SubagentId` for events that have one.
    /// `Message` events use `from_id`.
    pub fn subagent_id(&self) -> SubagentId {
        match self {
            BusEvent::Token { id, .. }
            | BusEvent::ToolCall { id, .. }
            | BusEvent::ToolResult { id, .. }
            | BusEvent::Done { id, .. }
            | BusEvent::Error { id, .. }
            | BusEvent::PermAsk { id, .. } => *id,
            BusEvent::Message { from_id, .. } => *from_id,
        }
    }
}

/// Sender half of the shared bus channel.
pub type BusSender = mpsc::Sender<BusEvent>;

/// Receiver half of the shared bus channel.
pub type BusReceiver = mpsc::Receiver<BusEvent>;

/// Create the shared bus channel pair.
pub fn create_bus() -> (BusSender, BusReceiver) {
    mpsc::channel(BUS_CAPACITY)
}

/// Spawn a relay task that reads `AgentEvent`s from an agent runner and
/// re-emits them as tagged `BusEvent`s onto the shared bus.
///
/// Returns the `JoinHandle` for the relay task. Store this on `SubagentHandle`
/// and call `handle.abort()` when the subagent is stopped.
///
/// The relay task exits cleanly when `event_rx` closes (sender dropped).
pub fn spawn_agent_relay(
    id: SubagentId,
    mut event_rx: mpsc::Receiver<AgentEvent>,
    bus_tx: BusSender,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // INVARIANT: This task is the sole relay for subagent `id`.
        // It exits when event_rx closes (runner dropped) or when aborted by stop().
        while let Some(event) = event_rx.recv().await {
            let bus_event = match event {
                AgentEvent::Token(text) => BusEvent::Token { id, text },
                AgentEvent::Reasoning(_) => continue, // reasoning not relayed to bus
                AgentEvent::ToolCall { name, args } => BusEvent::ToolCall { id, name, args },
                AgentEvent::ToolResult { output } => BusEvent::ToolResult { id, output },
                AgentEvent::Done {
                    response,
                    tokens,
                    cost,
                } => BusEvent::Done {
                    id,
                    response,
                    tokens,
                    cost,
                },
                AgentEvent::Error(message) => BusEvent::Error { id, message },
            };

            // If the bus receiver has been dropped, exit cleanly.
            if bus_tx.send(bus_event).await.is_err() {
                break;
            }
        }
    })
}

/// Spawn a relay task that bridges a subagent's permission ask channel onto the bus.
///
/// Returns the `JoinHandle`. Abort this when the subagent is stopped.
pub fn spawn_perm_relay(
    id: SubagentId,
    mut ask_rx: AskReceiver,
    bus_tx: BusSender,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(request) = ask_rx.recv().await {
            if bus_tx
                .send(BusEvent::PermAsk { id, request })
                .await
                .is_err()
            {
                break;
            }
        }
    })
}

//! Teams subsystem: group subagents into named teams with shared task boards.
//!
//! Feature-gated by `#[cfg(feature = "teams")]` (which implies `subagent`).
//! Builds on top of `SubagentRegistry` — teams do not own agents, they track them.

pub mod registry;
pub mod task_board;
pub mod team;
pub mod tools;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use registry::TeamRegistry;

/// Context passed to an agent (lead or subagent) when teams are enabled.
///
/// Holds a shared reference to the team registry (which itself encapsulates the
/// `SubagentRegistry`) so the LLM-callable tools can access them from within
/// async `Tool::call()` invocations. The `Arc` is cheap to clone into tool
/// instances.
///
/// `is_lead` controls which tier of tools is registered:
/// - `true`  → lead agent: receives all lead-only + shared tools
/// - `false` → subagent: receives only the shared communication tools
pub struct TeamContext {
    /// Registry of all named teams (encapsulates the `SubagentRegistry`).
    pub teams: Arc<TeamRegistry>,
    /// Whether this context is for the lead agent (true) or a team member subagent (false).
    pub is_lead: bool,
}

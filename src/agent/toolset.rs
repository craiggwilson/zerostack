//! Tool set configuration for agents.
//!
//! `ToolSet` controls which tools are registered when building an agent.
//! It is an unconditional type — available regardless of feature flags —
//! so that `build_agent_inner` has a uniform signature for all callers.
//!
//! The lead agent always uses `ToolSet::default()` (all tools, matching
//! pre-existing behavior). Subagents can use `ToolPreset::ReadOnly`,
//! `ToolPreset::None`, or per-tool overrides.

use std::collections::HashMap;

/// Preset tool availability for an agent.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // ReadOnly and None are used when subagent/teams features are enabled
pub enum ToolPreset {
    /// All tools (default behavior, no change for existing callers).
    #[default]
    All,
    /// Read-only tools only: read, grep, find_files, list_dir.
    ReadOnly,
    /// No tools registered — LLM-only responses.
    None,
}

/// Parameterized tool set for an agent.
///
/// `ToolSet::default()` produces exactly the current lead agent tool list,
/// preserving backward compatibility for all callers.
#[derive(Debug, Clone, Default)]
pub struct ToolSet {
    /// Base preset controlling which tools are included.
    pub preset: ToolPreset,
    /// Per-tool overrides applied on top of the preset.
    /// `true` = force-include, `false` = force-exclude.
    pub overrides: HashMap<String, bool>,
}

impl ToolSet {
    /// Returns `true` if this tool should be included, given preset + overrides.
    pub fn includes(&self, tool: &str) -> bool {
        if let Some(&override_val) = self.overrides.get(tool) {
            return override_val;
        }
        match self.preset {
            ToolPreset::All => true,
            ToolPreset::ReadOnly => matches!(tool, "read" | "grep" | "find_files" | "list_dir"),
            ToolPreset::None => false,
        }
    }

    /// Convenience: read-only tool set with no overrides.
    #[allow(dead_code)] // used when subagent feature is enabled
    pub fn read_only() -> Self {
        ToolSet {
            preset: ToolPreset::ReadOnly,
            overrides: HashMap::new(),
        }
    }

    /// Convenience: no-tools tool set.
    #[allow(dead_code)] // used when subagent feature is enabled
    pub fn no_tools() -> Self {
        ToolSet {
            preset: ToolPreset::None,
            overrides: HashMap::new(),
        }
    }
}

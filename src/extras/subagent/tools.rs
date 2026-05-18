//! LLM-callable agent tools for the lead agent.
//!
//! These tools cover agent lifecycle management: spawning, stopping, and status listing.
//!
//! # Async safety
//!
//! All `Tool::call()` implementations are fully synchronous internally.
//! No lock is ever held across an `.await` boundary.

use std::sync::Arc;

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;

use crate::agent::tools::ToolError;
use crate::extras::subagent::SubagentRegistry;

// ─── agent_spawn ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AgentSpawnArgs {
    pub name: String,
    pub prompt: String,
    #[serde(default)]
    pub readonly: bool,
    #[serde(default)]
    pub no_tools: bool,
}

/// Lead-only tool: spawn a new subagent.
///
/// Actual spawning requires the UI loop's full builder context (client, cli,
/// cfg…) which is unavailable inside `Tool::call()`. This tool validates name
/// uniqueness, then returns the `/agent spawn` slash command for the lead
/// runner to issue on the next turn.
pub struct AgentSpawnTool {
    pub registry: Arc<SubagentRegistry>,
}

impl Tool for AgentSpawnTool {
    const NAME: &'static str = "agent_spawn";
    type Error = ToolError;
    type Args = AgentSpawnArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "agent_spawn".to_string(),
            description: "Spawn a new subagent. Use readonly=true for read-only tool access, no_tools=true for no tool access. Returns the slash command to issue.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Unique name for the new subagent" },
                    "prompt": { "type": "string", "description": "Initial prompt / task for the subagent" },
                    "readonly": { "type": "boolean", "description": "If true, subagent gets read-only tools only" },
                    "no_tools": { "type": "boolean", "description": "If true, subagent gets no tools" }
                },
                "required": ["name", "prompt"]
            }),
        }
    }

    async fn call(&self, args: AgentSpawnArgs) -> Result<String, ToolError> {
        if self.registry.id_by_name(&args.name).is_some() {
            return Err(ToolError::Msg(format!(
                "subagent '{}' already exists. Choose a different name.",
                args.name
            )));
        }
        let preset_flag = if args.no_tools {
            " --no-tools"
        } else if args.readonly {
            " --readonly"
        } else {
            ""
        };
        Ok(format!(
            "To spawn subagent '{}': use `/agent spawn {}{} {}`",
            args.name, args.name, preset_flag, args.prompt
        ))
    }
}

// ─── agent_stop ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AgentStopArgs {
    pub name: String,
}

/// Lead-only tool: stop a named subagent.
pub struct AgentStopTool {
    pub registry: Arc<SubagentRegistry>,
}

impl Tool for AgentStopTool {
    const NAME: &'static str = "agent_stop";
    type Error = ToolError;
    type Args = AgentStopArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "agent_stop".to_string(),
            description: "Stop a named subagent. The subagent is removed from the registry and its relay tasks are aborted.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Name of the subagent to stop" }
                },
                "required": ["name"]
            }),
        }
    }

    async fn call(&self, args: AgentStopArgs) -> Result<String, ToolError> {
        let id = self
            .registry
            .id_by_name(&args.name)
            .ok_or_else(|| ToolError::Msg(format!("subagent '{}' not found", args.name)))?;
        self.registry
            .stop(id)
            .map_err(|e| ToolError::Msg(e.to_string()))?;
        Ok(format!("stopped subagent '{}'", args.name))
    }
}

// ─── agent_status ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AgentStatusArgs {}

/// Lead-only tool: list all subagents and their current status.
pub struct AgentStatusTool {
    pub registry: Arc<SubagentRegistry>,
}

impl Tool for AgentStatusTool {
    const NAME: &'static str = "agent_status";
    type Error = ToolError;
    type Args = AgentStatusArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "agent_status".to_string(),
            description: "List all active subagents with their id, name, and status (running/idle/done/error).".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    async fn call(&self, _args: AgentStatusArgs) -> Result<String, ToolError> {
        let agents = self.registry.list();
        if agents.is_empty() {
            return Ok("no subagents".to_string());
        }
        let lines: Vec<String> = agents
            .iter()
            .map(|s| {
                let summary = s
                    .final_response
                    .as_deref()
                    .map(|r| format!("  last: {}", &r[..r.len().min(60)]))
                    .unwrap_or_default();
                format!("  [{}] {} — {}{}", s.id, s.name, s.status, summary)
            })
            .collect();
        Ok(format!(
            "{} subagent(s):\n{}",
            agents.len(),
            lines.join("\n")
        ))
    }
}

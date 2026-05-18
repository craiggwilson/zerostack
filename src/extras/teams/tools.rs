//! LLM-callable team tools for the lead agent and team member subagents.
//!
//! Tools are split into two tiers:
//!
//! **Lead-only** (structural / destructive operations):
//! - `agent_spawn`   — spawn a new team member (defined in `subagent::tools`)
//! - `agent_stop`    — stop a named subagent (defined in `subagent::tools`)
//! - `agent_status`  — list all subagents and their status (defined in `subagent::tools`)
//! - `team_create`   — create a named team
//! - `team_disband`  — stop all members of a named team and remove it
//!
//! **Shared** (communication — lead and subagents both receive these):
//! - `agent_message` — send a message to a single named subagent's inbox
//! - `team_message`  — send a message to all members of a named team
//! - `team_status`   — snapshot of a named team's members and task board
//! - `team_tasks`    — add, complete, or list tasks on a named team's board
//!
//! All team tools require a `team` argument (the team name). The team is
//! looked up in the `TeamRegistry` at call time.
//!
//! # Lock discipline
//!
//! The `TeamRegistry` outer Mutex is held only to clone the `Arc<RwLock<Team>>`
//! — it is always released before any `RwLock<Team>` guard is acquired.
//! No lock is ever held across an `.await` boundary.

use std::sync::Arc;

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;

use super::registry::TeamRegistry;
use super::task_board::Priority;
use super::team::Team;
use crate::agent::tools::ToolError;
use crate::extras::subagent::SubagentRegistry;

// ─── helpers ─────────────────────────────────────────────────────────────────

fn lookup_team(
    registry: &TeamRegistry,
    name: &str,
) -> Result<Arc<std::sync::RwLock<Team>>, ToolError> {
    registry
        .get(name)
        .ok_or_else(|| ToolError::Msg(format!("team '{}' not found — use team_create first", name)))
}

// ─── team_create ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct TeamCreateArgs {
    pub team: String,
}

/// Lead-only tool: create a new named team.
pub struct TeamCreateTool {
    pub team_registry: Arc<TeamRegistry>,
}

impl Tool for TeamCreateTool {
    const NAME: &'static str = "team_create";
    type Error = ToolError;
    type Args = TeamCreateArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "team_create".to_string(),
            description:
                "Create a new named team. Returns an error if a team with that name already exists."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "team": { "type": "string", "description": "Name for the new team" }
                },
                "required": ["team"]
            }),
        }
    }

    async fn call(&self, args: TeamCreateArgs) -> Result<String, ToolError> {
        self.team_registry
            .create(args.team.clone())
            .map(|_| format!("team '{}' created", args.team))
            .map_err(|e| ToolError::Msg(e.to_string()))
    }
}

// ─── team_spawn ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct TeamSpawnArgs {
    pub team: String,
    pub name: String,
    pub prompt: String,
    #[serde(default)]
    pub readonly: bool,
    #[serde(default)]
    pub no_tools: bool,
}

/// Lead-only tool: spawn a new subagent and assign it to a named team.
///
/// Actual spawning requires the UI loop's full builder context (client, cli,
/// cfg…) which is unavailable inside `Tool::call()`. This tool validates that
/// the team exists and the name is unique, then returns the `/team spawn` slash
/// command for the lead runner to issue on the next turn.
pub struct TeamSpawnTool {
    pub registry: Arc<SubagentRegistry>,
    pub team_registry: Arc<TeamRegistry>,
}

impl Tool for TeamSpawnTool {
    const NAME: &'static str = "team_spawn";
    type Error = ToolError;
    type Args = TeamSpawnArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "team_spawn".to_string(),
            description: "Spawn a new subagent and assign it to a named team. Use team_create first if the team doesn't exist. Use readonly=true for read-only tool access, no_tools=true for no tool access. Returns the slash command to issue.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "team": { "type": "string", "description": "Name of the team to assign the subagent to" },
                    "name": { "type": "string", "description": "Unique name for the new subagent" },
                    "prompt": { "type": "string", "description": "Initial prompt / task for the subagent" },
                    "readonly": { "type": "boolean", "description": "If true, subagent gets read-only tools only" },
                    "no_tools": { "type": "boolean", "description": "If true, subagent gets no tools" }
                },
                "required": ["team", "name", "prompt"]
            }),
        }
    }

    async fn call(&self, args: TeamSpawnArgs) -> Result<String, ToolError> {
        if self.team_registry.get(&args.team).is_none() {
            return Err(ToolError::Msg(format!(
                "team '{}' not found — use team_create first",
                args.team
            )));
        }
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
            "To spawn subagent '{}' into team '{}': use `/team spawn {} {}{} {}`",
            args.name, args.team, args.team, args.name, preset_flag, args.prompt
        ))
    }
}

// ─── team_disband ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct TeamDisbandArgs {
    pub team: String,
}

/// Lead-only tool: stop all members of a named team and remove it from the registry.
pub struct TeamDisbandTool {
    pub team_registry: Arc<TeamRegistry>,
}

impl Tool for TeamDisbandTool {
    const NAME: &'static str = "team_disband";
    type Error = ToolError;
    type Args = TeamDisbandArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "team_disband".to_string(),
            description: "Stop all members of a named team and remove it from the registry."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "team": { "type": "string", "description": "Name of the team to disband" }
                },
                "required": ["team"]
            }),
        }
    }

    async fn call(&self, args: TeamDisbandArgs) -> Result<String, ToolError> {
        let member_ids = {
            let arc = lookup_team(&self.team_registry, &args.team)?;
            let guard = arc.read().unwrap_or_else(|e| e.into_inner());
            guard.members().to_vec()
        };
        let agents = self.team_registry.agents();
        let mut stopped = 0usize;
        let mut errors: Vec<String> = Vec::new();
        for id in &member_ids {
            match agents.stop(*id) {
                Ok(()) => stopped += 1,
                Err(e) => errors.push(e.to_string()),
            }
        }
        self.team_registry
            .remove(&args.team)
            .map_err(|e| ToolError::Msg(e.to_string()))?;
        if errors.is_empty() {
            Ok(format!(
                "disbanded team '{}': stopped {} member(s)",
                args.team, stopped
            ))
        } else {
            Ok(format!(
                "disbanded team '{}': stopped {} member(s), {} error(s): {}",
                args.team,
                stopped,
                errors.len(),
                errors.join("; ")
            ))
        }
    }
}

// ─── agent_message ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AgentMessageArgs {
    pub name: String,
    pub message: String,
}

/// Shared tool (lead + subagents): send a message to a single named subagent's inbox.
pub struct AgentMessageTool {
    pub agents: Arc<SubagentRegistry>,
}

impl Tool for AgentMessageTool {
    const NAME: &'static str = "agent_message";
    type Error = ToolError;
    type Args = AgentMessageArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "agent_message".to_string(),
            description: "Send a message to a specific subagent by name. Use team_message to send to all members of a team.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Name of the target subagent" },
                    "message": { "type": "string", "description": "Message content to send" }
                },
                "required": ["name", "message"]
            }),
        }
    }

    async fn call(&self, args: AgentMessageArgs) -> Result<String, ToolError> {
        let id = self
            .agents
            .id_by_name(&args.name)
            .ok_or_else(|| ToolError::Msg(format!("subagent '{}' not found", args.name)))?;
        self.agents
            .send_message(id, args.message)
            .map_err(|e| ToolError::Msg(e.to_string()))?;
        Ok(format!("message sent to '{}'", args.name))
    }
}

// ─── team_message ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct TeamMessageArgs {
    pub team: String,
    pub message: String,
}

/// Shared tool (lead + subagents): send a message to all members of a named team.
pub struct TeamMessageTool {
    pub team_registry: Arc<TeamRegistry>,
}

impl Tool for TeamMessageTool {
    const NAME: &'static str = "team_message";
    type Error = ToolError;
    type Args = TeamMessageArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "team_message".to_string(),
            description: "Send a message to all members of a named team simultaneously. Use agent_message to target a single agent.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "team": { "type": "string", "description": "Name of the team" },
                    "message": { "type": "string", "description": "Message to send to all team members" }
                },
                "required": ["team", "message"]
            }),
        }
    }

    async fn call(&self, args: TeamMessageArgs) -> Result<String, ToolError> {
        let delivered = {
            let arc = lookup_team(&self.team_registry, &args.team)?;
            let guard = arc.read().unwrap_or_else(|e| e.into_inner());
            guard.message(args.message)
        };
        Ok(format!(
            "message delivered to {} member(s) of team '{}'",
            delivered, args.team
        ))
    }
}

// ─── team_status ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct TeamStatusArgs {
    pub team: String,
}

/// Shared tool (lead + subagents): snapshot of a named team's members and task board.
pub struct TeamStatusTool {
    pub team_registry: Arc<TeamRegistry>,
}

impl Tool for TeamStatusTool {
    const NAME: &'static str = "team_status";
    type Error = ToolError;
    type Args = TeamStatusArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "team_status".to_string(),
            description: "Get a formatted snapshot of a named team's members (name, id, status) and task board summary. Use team_list to see all teams.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "team": { "type": "string", "description": "Name of the team" }
                },
                "required": ["team"]
            }),
        }
    }

    async fn call(&self, args: TeamStatusArgs) -> Result<String, ToolError> {
        let (member_ids, task_summary) = {
            let arc = lookup_team(&self.team_registry, &args.team)?;
            let guard = arc.read().unwrap_or_else(|e| e.into_inner());
            (guard.members().to_vec(), guard.task_board_summary())
        };

        let snapshots = self.team_registry.agents().list();
        let mut lines = vec![format!(
            "team '{}' ({} members):",
            args.team,
            member_ids.len()
        )];
        for id in &member_ids {
            if let Some(snap) = snapshots.iter().find(|s| s.id == *id) {
                lines.push(format!("  [{}] {} — {}", snap.id, snap.name, snap.status));
            }
        }
        lines.push(format!(
            "tasks: {}/{} done, {} pending, {} blocked",
            task_summary.done, task_summary.total, task_summary.pending, task_summary.blocked
        ));
        Ok(lines.join("\n"))
    }
}

// ─── team_list ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct TeamListArgs {}

/// Shared tool (lead + subagents): list all teams in the registry.
pub struct TeamListTool {
    pub team_registry: Arc<TeamRegistry>,
}

impl Tool for TeamListTool {
    const NAME: &'static str = "team_list";
    type Error = ToolError;
    type Args = TeamListArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "team_list".to_string(),
            description: "List all active teams by name.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    async fn call(&self, _args: TeamListArgs) -> Result<String, ToolError> {
        let names = self.team_registry.list();
        if names.is_empty() {
            Ok("no teams".to_string())
        } else {
            Ok(format!("{} team(s): {}", names.len(), names.join(", ")))
        }
    }
}

// ─── team_tasks ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct TeamTasksArgs {
    pub team: String,
    /// "add", "done", or "list"
    pub action: String,
    /// Task content (for "add")
    pub content: Option<String>,
    /// Priority: "high", "medium", or "low" (for "add"; defaults to "medium")
    pub priority: Option<String>,
    /// Task id (for "done")
    pub id: Option<u32>,
}

/// Shared tool (lead + subagents): manage a named team's task board.
pub struct TeamTasksTool {
    pub team_registry: Arc<TeamRegistry>,
}

impl Tool for TeamTasksTool {
    const NAME: &'static str = "team_tasks";
    type Error = ToolError;
    type Args = TeamTasksArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "team_tasks".to_string(),
            description: "Manage a named team's task board. Actions: 'add' (requires content, optional priority: high/medium/low), 'done' (requires id), 'list'.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "team": { "type": "string", "description": "Name of the team" },
                    "action": { "type": "string", "enum": ["add", "done", "list"], "description": "Task board action" },
                    "content": { "type": "string", "description": "Task description (required for 'add')" },
                    "priority": { "type": "string", "enum": ["high", "medium", "low"], "description": "Task priority (optional, for 'add')" },
                    "id": { "type": "integer", "description": "Task id (required for 'done')" }
                },
                "required": ["team", "action"]
            }),
        }
    }

    async fn call(&self, args: TeamTasksArgs) -> Result<String, ToolError> {
        let arc = lookup_team(&self.team_registry, &args.team)?;
        match args.action.as_str() {
            "add" => {
                let content = args
                    .content
                    .ok_or_else(|| ToolError::Msg("'add' requires 'content'".to_string()))?;
                let priority = match args.priority.as_deref().unwrap_or("medium") {
                    "high" => Priority::High,
                    "low" => Priority::Low,
                    _ => Priority::Medium,
                };
                let task_id = {
                    let mut guard = arc.write().unwrap_or_else(|e| e.into_inner());
                    guard.add_task(content, priority, vec![])
                };
                Ok(format!("task {} added to team '{}'", task_id, args.team))
            }
            "done" => {
                let id = args
                    .id
                    .ok_or_else(|| ToolError::Msg("'done' requires 'id'".to_string()))?;
                {
                    let mut guard = arc.write().unwrap_or_else(|e| e.into_inner());
                    guard
                        .mark_task_done(super::task_board::TaskId(id))
                        .map_err(|e| ToolError::Msg(e.to_string()))?;
                }
                Ok(format!("task {} marked done", id))
            }
            "list" => {
                let output = {
                    let guard = arc.read().unwrap_or_else(|e| e.into_inner());
                    let tasks: Vec<_> = guard
                        .list_tasks()
                        .into_iter()
                        .map(|t| {
                            (
                                t.id,
                                t.content.clone(),
                                t.status.to_string(),
                                t.priority.to_string(),
                            )
                        })
                        .collect();
                    let summary = guard.task_board_summary();
                    (tasks, summary)
                };
                let (tasks, summary) = output;
                if tasks.is_empty() {
                    Ok(format!("no tasks in team '{}'", args.team))
                } else {
                    let mut lines = vec![format!(
                        "team '{}' tasks: {} total, {} done, {} pending, {} blocked, {} in_progress",
                        args.team,
                        summary.total,
                        summary.done,
                        summary.pending,
                        summary.blocked,
                        summary.in_progress,
                    )];
                    for (id, content, status, priority) in tasks {
                        lines.push(format!(
                            "  [{}] {} — {} ({})",
                            id, content, status, priority
                        ));
                    }
                    Ok(lines.join("\n"))
                }
            }
            other => Err(ToolError::Msg(format!(
                "unknown action '{}' — use 'add', 'done', or 'list'",
                other
            ))),
        }
    }
}

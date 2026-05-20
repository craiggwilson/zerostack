use rig::completion::ToolDefinition;
use tokio::time::{Duration, timeout};

use crate::agent::tools::{BashArgs, ContextualTool, ToolContext, ToolError, ToolName};

pub struct BashTool;

impl ContextualTool for BashTool {
    type Args = BashArgs;
    type Output = String;
    type Error = ToolError;

    fn name(&self) -> ToolName {
        ToolName::bash()
    }

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "bash".to_string(),
            description: "Execute a bash command in the current working directory. Returns stdout and stderr.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Bash command to execute" },
                    "timeout": { "type": "integer", "description": "Timeout in seconds (optional)" }
                },
                "required": ["command"]
            }),
        }
    }

    async fn call(&self, ctx: &ToolContext, args: BashArgs) -> Result<String, ToolError> {
        ctx.check_perm(&ToolName::bash(), &args.command).await?;

        let output = if let Some(secs) = args.timeout {
            timeout(
                Duration::from_secs(secs),
                ctx.sandbox.wrap_command(&args.command).output(),
            )
            .await
            .map_err(|_| ToolError::Msg("Command timed out".to_string()))?
        } else {
            ctx.sandbox.wrap_command(&args.command).output().await
        }?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let exit_code = output.status.code().unwrap_or(-1);

        let mut result = String::new();
        if !stdout.is_empty() {
            result.push_str(&stdout);
        }
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(&stderr);
        }
        if exit_code != 0 {
            result.push_str(&format!("\nExit code: {}", exit_code));
        }
        Ok(result)
    }
}

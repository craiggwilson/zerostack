use rig::completion::ToolDefinition;

use crate::agent::tools::{ContextualTool, ReadArgs, ToolContext, ToolError, ToolName};

pub struct ReadTool;

impl ContextualTool for ReadTool {
    type Args = ReadArgs;
    type Output = String;
    type Error = ToolError;

    fn name(&self) -> ToolName {
        ToolName::read()
    }

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "read".to_string(),
            description: "Read the contents of a file. Supports text files. Defaults to first 2000 lines. Use offset/limit for large files.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file (relative or absolute)" },
                    "offset": { "type": "integer", "description": "Line number to start from (1-indexed)" },
                    "limit": { "type": "integer", "description": "Maximum number of lines to read" }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, ctx: &ToolContext, args: ReadArgs) -> Result<String, ToolError> {
        ctx.check_perm_path(&ToolName::read(), &args.path).await?;

        let metadata = tokio::fs::metadata(&args.path).await?;
        let file_size = metadata.len();
        if file_size > 10 * 1024 * 1024 {
            return Err(ToolError::Msg(format!(
                "File too large ({} bytes). Max 10MB.",
                file_size
            )));
        }
        let content = tokio::fs::read_to_string(&args.path).await?;
        let total_lines = content.lines().count();

        let offset = args.offset.unwrap_or(1).max(1) - 1;
        let limit = args.limit.unwrap_or(2000);
        let end = (offset + limit).min(total_lines);

        let excerpt: String = content
            .lines()
            .skip(offset)
            .take(end - offset)
            .collect::<Vec<_>>()
            .join("\n");
        let info = format!(
            "File: {} ({} lines total, showing lines {}-{})\n\n{}",
            args.path,
            total_lines,
            offset + 1,
            end,
            excerpt
        );
        Ok(info)
    }
}

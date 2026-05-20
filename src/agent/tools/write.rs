use std::path::Path;

use rig::completion::ToolDefinition;

use crate::agent::tools::{ContextualTool, ToolContext, ToolError, ToolName, WriteArgs};

pub struct WriteTool;

impl ContextualTool for WriteTool {
    type Args = WriteArgs;
    type Output = String;
    type Error = ToolError;

    fn name(&self) -> ToolName {
        ToolName::write()
    }

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "write".to_string(),
            description: "Write content to a file. Creates the file if it doesn't exist, overwrites if it does. Automatically creates parent directories.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file (relative or absolute)" },
                    "content": { "type": "string", "description": "Content to write to the file" }
                },
                "required": ["path", "content"]
            }),
        }
    }

    async fn call(&self, ctx: &ToolContext, args: WriteArgs) -> Result<String, ToolError> {
        ctx.check_perm_path(&ToolName::write(), &args.path).await?;

        let path = Path::new(&args.path);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let bytes = args.content.len();
        tokio::fs::write(path, &args.content).await?;
        Ok(format!("Written {} bytes to {}", bytes, args.path))
    }
}

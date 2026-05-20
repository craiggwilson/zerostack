use std::sync::Arc;

use compact_str::CompactString;
use rig::agent::{Agent, AgentBuilder};
use rig::completion::CompletionModel;
use rig::providers::openrouter;

use crate::agent::prompt::{SYSTEM_PROMPT, TODO_TOOLS_PROMPT};
use crate::agent::tools::ToolContext;
use crate::agent::toolset::ToolSet;
use crate::cli::Cli;
use crate::config::Config;
use crate::context::ContextFiles;

#[allow(dead_code)]
pub type ZAgent = Agent<openrouter::CompletionModel>;

/// Build an agent with the given model, configuration, and tool set.
///
/// The caller is responsible for constructing the appropriate `ToolSet`
/// (including honouring `--no-tools` and registering MCP tools). This
/// function trusts the `ToolSet` it receives.
pub async fn build_agent_inner<M: CompletionModel + 'static>(
    model: M,
    cli: &Cli,
    cfg: &Config,
    context: &ContextFiles,
    ctx: &Arc<ToolContext>,
    tool_set: &ToolSet,
) -> Agent<M> {
    let mut preamble = SYSTEM_PROMPT.to_string();
    preamble.push('\n');
    preamble.push_str(TODO_TOOLS_PROMPT);
    if let Some(agents) = &context.agents {
        preamble.push_str("\n\n");
        preamble.push_str(agents);
    }

    if let Some(prompt) = &context.current_prompt {
        preamble.push_str("\n\n---\n\n");
        preamble.push_str(prompt);
    }

    if let Ok(cwd) = std::env::current_dir() {
        let cwd_str = cwd.display();
        preamble.push_str(&format!("\n\nCurrent working directory: {}", cwd_str));
    }

    let mut builder = AgentBuilder::new(model).preamble(&preamble);

    let max_tokens = cli.resolve_max_tokens(cfg);
    builder = builder.max_tokens(max_tokens);

    let max_turns = cli.resolve_max_agent_turns(cfg);
    builder = builder.default_max_turns(max_turns);

    if let Some(temp) = cli.temperature {
        let clamped = temp.clamp(0.0, 2.0);
        builder = builder.temperature(clamped);
    }

    let tools = tool_set.tools(ctx);
    builder.tools(tools).build()
}

#[allow(dead_code)]
pub fn create_client(api_key: Option<&str>) -> anyhow::Result<openrouter::Client> {
    let key = api_key
        .map(CompactString::new)
        .or_else(|| {
            std::env::var("OPENROUTER_API_KEY")
                .ok()
                .map(CompactString::new)
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No API key found. Set OPENROUTER_API_KEY environment variable or pass --api-key."
            )
        })?;
    Ok(openrouter::Client::new(String::from(key))?)
}

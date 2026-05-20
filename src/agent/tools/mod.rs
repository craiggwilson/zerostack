mod bash;
pub(crate) mod edit;
mod find_files;
mod grep;
mod list_dir;
mod read;
mod todo;
mod write;

pub use bash::BashTool;
pub use edit::EditTool;
pub use find_files::FindFilesTool;
pub use grep::GrepTool;
pub use list_dir::ListDirTool;
pub use read::ReadTool;
pub use todo::WriteTodoList;
pub use write::WriteTool;

use std::io;
use std::sync::Arc;

use compact_str::CompactString;
use serde::{Deserialize, Serialize};

use crate::permission::ask::{AskRequest, AskSender, UserDecision};
use crate::permission::checker::{CheckResult, PermCheck};
use crate::sandbox::Sandbox;

pub const MAX_GREP_RESULTS: usize = 200;
pub const MAX_FIND_RESULTS: usize = 200;

// ── ToolName ─────────────────────────────────────────────────────────────────

/// Type-safe tool name newtype. Wraps `CompactString` for small-string
/// optimisation. Serialises/deserialises as a plain JSON string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ToolName(pub CompactString);

impl ToolName {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    // Well-known constants for the 8 built-in tools.
    pub fn bash() -> Self {
        ToolName(CompactString::new("bash"))
    }
    pub fn read() -> Self {
        ToolName(CompactString::new("read"))
    }
    pub fn write() -> Self {
        ToolName(CompactString::new("write"))
    }
    pub fn edit() -> Self {
        ToolName(CompactString::new("edit"))
    }
    pub fn grep() -> Self {
        ToolName(CompactString::new("grep"))
    }
    pub fn find_files() -> Self {
        ToolName(CompactString::new("find_files"))
    }
    pub fn list_dir() -> Self {
        ToolName(CompactString::new("list_dir"))
    }
    pub fn write_todo_list() -> Self {
        ToolName(CompactString::new("write_todo_list"))
    }

    // Associated constants exposed as `ToolName::BASH`, etc.
    // We use lazy_static because CompactString doesn't support const construction.
}

// Expose uppercase constant-style accessors on the type itself.
impl ToolName {
    pub const BASH: LazyToolName = LazyToolName("bash");
    pub const READ: LazyToolName = LazyToolName("read");
    pub const WRITE: LazyToolName = LazyToolName("write");
    pub const EDIT: LazyToolName = LazyToolName("edit");
    pub const GREP: LazyToolName = LazyToolName("grep");
    pub const FIND_FILES: LazyToolName = LazyToolName("find_files");
    pub const LIST_DIR: LazyToolName = LazyToolName("list_dir");
    pub const WRITE_TODO_LIST: LazyToolName = LazyToolName("write_todo_list");
}

/// Helper that lets `ToolName::BASH` coerce to a `ToolName` via `Deref`
/// and compares equal to `ToolName::from("bash")`.
pub struct LazyToolName(pub &'static str);

impl LazyToolName {
    pub fn get(&self) -> ToolName {
        ToolName(CompactString::new(self.0))
    }
    pub fn as_str(&self) -> &str {
        self.0
    }
}

impl PartialEq<ToolName> for LazyToolName {
    fn eq(&self, other: &ToolName) -> bool {
        self.0 == other.0.as_str()
    }
}

impl PartialEq<LazyToolName> for ToolName {
    fn eq(&self, other: &LazyToolName) -> bool {
        self.0.as_str() == other.0
    }
}

impl std::fmt::Display for LazyToolName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&'static LazyToolName> for ToolName {
    fn from(l: &'static LazyToolName) -> Self {
        ToolName(CompactString::new(l.0))
    }
}

impl std::fmt::Display for ToolName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for ToolName {
    fn from(s: &str) -> Self {
        ToolName(CompactString::new(s))
    }
}

impl From<String> for ToolName {
    fn from(s: String) -> Self {
        ToolName(CompactString::new(s))
    }
}

impl From<CompactString> for ToolName {
    fn from(s: CompactString) -> Self {
        ToolName(s)
    }
}

// ── ToolContext ───────────────────────────────────────────────────────────────

/// Consolidated context passed to every tool call. Replaces the
/// `permission`, `ask_tx`, `sandbox` triple that was stored per-tool.
pub struct ToolContext {
    pub permission: Option<PermCheck>,
    pub ask_tx: Option<AskSender>,
    pub sandbox: Sandbox,
}

impl ToolContext {
    /// Full interactive context with permission checking and ask channel.
    pub fn interactive(
        permission: Option<PermCheck>,
        ask_tx: Option<AskSender>,
        sandbox: Sandbox,
    ) -> Arc<Self> {
        Arc::new(ToolContext {
            permission,
            ask_tx,
            sandbox,
        })
    }

    /// Headless context — all operations are allowed.
    pub fn headless(sandbox: Sandbox) -> Arc<Self> {
        Arc::new(ToolContext {
            permission: None,
            ask_tx: None,
            sandbox,
        })
    }

    /// Check permission for a tool call using a generic input key.
    pub async fn check_perm(&self, tool: &ToolName, input_key: &str) -> Result<(), ToolError> {
        check_perm(&self.permission, &self.ask_tx, tool.as_str(), input_key).await
    }

    /// Check permission for a tool call that operates on a filesystem path.
    pub async fn check_perm_path(&self, tool: &ToolName, path: &str) -> Result<(), ToolError> {
        check_perm_path(&self.permission, &self.ask_tx, tool.as_str(), path).await
    }
}

// ── ContextualTool ────────────────────────────────────────────────────────────

/// Our trait for stateless tools. `ctx` is always the first param after `&self`.
pub trait ContextualTool: Send + Sync + 'static {
    type Args: for<'de> Deserialize<'de> + Send;
    type Output: Serialize + Send;
    type Error: std::error::Error + Send + Sync + 'static;

    fn name(&self) -> ToolName;

    fn definition(
        &self,
        prompt: String,
    ) -> impl std::future::Future<Output = rig::completion::ToolDefinition> + Send;

    fn call(
        &self,
        ctx: &ToolContext,
        args: Self::Args,
    ) -> impl std::future::Future<Output = Result<Self::Output, Self::Error>> + Send;
}

/// Object-safe dynamic version of ContextualTool.
pub trait ContextualToolDyn: Send + Sync {
    fn name_dyn(&self) -> ToolName;

    fn definition_dyn<'a>(
        &'a self,
        prompt: String,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = rig::completion::ToolDefinition> + Send + 'a>,
    >;

    fn call_dyn<'a>(
        &'a self,
        ctx: &'a ToolContext,
        args: String,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, ToolError>> + Send + 'a>>;
}

// Blanket impl of ContextualToolDyn for all ContextualTool implementors.
impl<T> ContextualToolDyn for T
where
    T: ContextualTool,
    T::Output: std::fmt::Display,
    T::Error: Into<ToolError>,
{
    fn name_dyn(&self) -> ToolName {
        self.name()
    }

    fn definition_dyn<'a>(
        &'a self,
        prompt: String,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = rig::completion::ToolDefinition> + Send + 'a>,
    > {
        Box::pin(self.definition(prompt))
    }

    fn call_dyn<'a>(
        &'a self,
        ctx: &'a ToolContext,
        args: String,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, ToolError>> + Send + 'a>>
    {
        Box::pin(async move {
            let parsed: T::Args =
                serde_json::from_str(&args).map_err(|e| ToolError::Msg(e.to_string()))?;
            let output = self.call(ctx, parsed).await.map_err(|e| e.into())?;
            Ok(output.to_string())
        })
    }
}

// ── BoundTool ─────────────────────────────────────────────────────────────────

/// Wraps a `ContextualToolDyn` + `Arc<ToolContext>` and implements rig's
/// `ToolDyn` so it can be passed to `AgentBuilder::tools()`.
pub struct BoundTool {
    pub inner: Box<dyn ContextualToolDyn>,
    pub ctx: Arc<ToolContext>,
}

impl rig::tool::ToolDyn for BoundTool {
    fn name(&self) -> String {
        self.inner.name_dyn().as_str().to_string()
    }

    fn definition(
        &self,
        prompt: String,
    ) -> rig::wasm_compat::WasmBoxedFuture<'_, rig::completion::ToolDefinition> {
        Box::pin(self.inner.definition_dyn(prompt))
    }

    fn call(
        &self,
        args: String,
    ) -> rig::wasm_compat::WasmBoxedFuture<'_, Result<String, rig::tool::ToolError>> {
        let ctx = self.ctx.as_ref();
        Box::pin(async move {
            self.inner
                .call_dyn(ctx, args)
                .await
                .map_err(|e| rig::tool::ToolError::ToolCallError(Box::new(e)))
        })
    }
}

// ── ToolError ─────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("{0}")]
    Msg(String),
}

impl From<io::Error> for ToolError {
    fn from(e: io::Error) -> Self {
        ToolError::Msg(e.to_string())
    }
}

impl From<serde_json::Error> for ToolError {
    fn from(e: serde_json::Error) -> Self {
        ToolError::Msg(e.to_string())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

pub fn is_skip_dir(name: &str) -> bool {
    matches!(name, "node_modules" | "target")
}

// ── Shared arg types ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ReadArgs {
    pub path: String,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Deserialize)]
pub struct WriteArgs {
    pub path: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct EditArgs {
    pub path: String,
    pub old_text: String,
    pub new_text: String,
    pub replace_all: Option<bool>,
}

#[derive(Deserialize)]
pub struct BashArgs {
    pub command: String,
    pub timeout: Option<u64>,
}

#[derive(Deserialize)]
pub struct GrepArgs {
    pub pattern: String,
    pub path: Option<String>,
    pub include: Option<String>,
    pub context_lines: Option<usize>,
}

#[derive(Deserialize)]
pub struct FindFilesArgs {
    pub pattern: String,
    pub path: Option<String>,
}

#[derive(Deserialize)]
pub struct ListDirArgs {
    pub path: Option<String>,
}

// ── Permission helpers (kept for MCP tool backward compat) ───────────────────

async fn handle_ask_inner(
    ask_tx: &AskSender,
    permission: &PermCheck,
    tool: &str,
    input: &str,
) -> Result<(), ToolError> {
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    ask_tx
        .send(AskRequest {
            tool: tool.to_string(),
            input: input.to_string(),
            reply: reply_tx,
        })
        .await
        .map_err(|_| ToolError::Msg("Permission system unavailable".to_string()))?;
    match reply_rx.await {
        Ok(UserDecision::AllowOnce) => Ok(()),
        Ok(UserDecision::AllowAlways(pattern)) => {
            permission
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .add_session_allowlist(tool.to_string(), &pattern);
            Ok(())
        }
        _ => Err(ToolError::Msg("Permission denied by user".to_string())),
    }
}

pub async fn check_perm(
    permission: &Option<PermCheck>,
    ask_tx: &Option<AskSender>,
    tool: &str,
    input_key: &str,
) -> Result<(), ToolError> {
    let Some(perm) = permission else {
        return Ok(());
    };
    let result = {
        let mut guard = perm.lock().unwrap_or_else(|e| e.into_inner());
        guard.check(tool, input_key)
    };
    match result {
        CheckResult::Allowed => Ok(()),
        CheckResult::Denied(reason) => {
            Err(ToolError::Msg(format!("Permission denied: {}", reason)))
        }
        CheckResult::Ask => {
            let Some(tx) = ask_tx else {
                return Err(ToolError::Msg(
                    "Permission denied (non-interactive mode)".to_string(),
                ));
            };
            handle_ask_inner(tx, perm, tool, input_key).await
        }
    }
}

pub async fn check_perm_path(
    permission: &Option<PermCheck>,
    ask_tx: &Option<AskSender>,
    tool: &str,
    path: &str,
) -> Result<(), ToolError> {
    let Some(perm) = permission else {
        return Ok(());
    };
    let result = {
        let mut guard = perm.lock().unwrap_or_else(|e| e.into_inner());
        guard.check_path(tool, path)
    };
    match result {
        CheckResult::Allowed => Ok(()),
        CheckResult::Denied(reason) => {
            Err(ToolError::Msg(format!("Permission denied: {}", reason)))
        }
        CheckResult::Ask => {
            let Some(tx) = ask_tx else {
                return Err(ToolError::Msg(
                    "Permission denied (non-interactive mode)".to_string(),
                ));
            };
            handle_ask_inner(tx, perm, tool, path).await
        }
    }
}

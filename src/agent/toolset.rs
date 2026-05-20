//! Tool set configuration for agents.
//!
//! `ToolSet` controls which tools are available when building an agent.
//! It owns a private `ToolRegistry` of all known tool factories and uses
//! a simple include/exclude model to filter them.
//!
//! The lead agent uses `ToolSet::default()` (all tools). Callers can
//! use `ToolSet::read_only()`, `ToolSet::no_tools()`, or per-tool
//! overrides via `with_tool()` / `without_tool()`.

use std::collections::HashMap;
use std::sync::Arc;

use crate::agent::tools::{
    BashTool, BoundTool, ContextualToolDyn, EditTool, FindFilesTool, GrepTool, ListDirTool,
    ReadTool, ToolContext, ToolName, WriteTodoList, WriteTool,
};

// ── ToolRegistry (private) ────────────────────────────────────────────────────

/// A factory that produces a `Box<dyn ToolDyn>` given a `ToolContext`.
///
/// Built-in tools receive the context and get wrapped in `BoundTool`.
/// Pre-built tools (e.g. MCP) already have their context captured and
/// ignore the argument.
type ToolFactory = Box<dyn Fn(&Arc<ToolContext>) -> Box<dyn rig::tool::ToolDyn> + Send + Sync>;

/// Wraps a `ContextualToolDyn` factory so it produces a `BoundTool` at
/// retrieval time.
fn contextual_factory<F>(f: F) -> ToolFactory
where
    F: Fn() -> Box<dyn ContextualToolDyn> + Send + Sync + 'static,
{
    Box::new(move |ctx: &Arc<ToolContext>| {
        Box::new(BoundTool {
            inner: f(),
            ctx: ctx.clone(),
        }) as Box<dyn rig::tool::ToolDyn>
    })
}

struct ToolRegistry {
    tools: HashMap<ToolName, ToolFactory>,
}

impl ToolRegistry {
    fn new() -> Self {
        ToolRegistry {
            tools: HashMap::new(),
        }
    }

    fn register(&mut self, name: ToolName, factory: ToolFactory) {
        self.tools.insert(name, factory);
    }

    fn with_all_builtins() -> Self {
        let mut r = ToolRegistry::new();
        r.register(ToolName::bash(), contextual_factory(|| Box::new(BashTool)));
        r.register(ToolName::read(), contextual_factory(|| Box::new(ReadTool)));
        r.register(
            ToolName::write(),
            contextual_factory(|| Box::new(WriteTool)),
        );
        r.register(ToolName::edit(), contextual_factory(|| Box::new(EditTool)));
        r.register(ToolName::grep(), contextual_factory(|| Box::new(GrepTool)));
        r.register(
            ToolName::find_files(),
            contextual_factory(|| Box::new(FindFilesTool)),
        );
        r.register(
            ToolName::list_dir(),
            contextual_factory(|| Box::new(ListDirTool)),
        );
        r.register(
            ToolName::write_todo_list(),
            contextual_factory(|| Box::new(WriteTodoList)),
        );
        r
    }

    fn create(
        &self,
        ctx: &Arc<ToolContext>,
        name: &ToolName,
    ) -> Option<Box<dyn rig::tool::ToolDyn>> {
        self.tools.get(name).map(|f| f(ctx))
    }

    fn names(&self) -> impl Iterator<Item = &ToolName> {
        self.tools.keys()
    }
}

// ── ToolSet ───────────────────────────────────────────────────────────────────

/// Controls which tools an agent has access to.
///
/// The filter is fully encapsulated in a `HashMap<ToolName, bool>`.
/// Constructors populate it appropriately:
/// - `default()` — all registered builtins included
/// - `no_tools()` — nothing included
/// - `read_only()` — only read, grep, find_files, list_dir
///
/// Use `with_tool()`, `without_tool()`, or `merge()` to customise.
pub struct ToolSet {
    /// Tool filter. `true` = included, `false` = excluded.
    /// Tools not present in the map are not included.
    filter: HashMap<ToolName, bool>,
    /// Private registry of tool factories.
    registry: ToolRegistry,
}

impl std::fmt::Debug for ToolSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolSet")
            .field("filter", &self.filter)
            .finish()
    }
}

impl Default for ToolSet {
    /// All registered tools included.
    fn default() -> Self {
        let registry = ToolRegistry::with_all_builtins();
        let filter = registry.names().map(|n| (n.clone(), true)).collect();
        ToolSet { filter, registry }
    }
}

impl ToolSet {
    /// Returns `true` if this tool is included.
    pub fn includes(&self, tool: &ToolName) -> bool {
        self.filter.get(tool).copied().unwrap_or(false)
    }

    /// Return all included tools, bound with the given context.
    pub fn tools(&self, ctx: &Arc<ToolContext>) -> Vec<Box<dyn rig::tool::ToolDyn>> {
        self.registry
            .names()
            .filter(|name| self.includes(name))
            .filter_map(|name| self.registry.create(ctx, name))
            .collect()
    }

    /// Return a single tool bound with the given context, or `None` if
    /// not included or not registered.
    pub fn get(
        &self,
        ctx: &Arc<ToolContext>,
        name: &ToolName,
    ) -> Option<Box<dyn rig::tool::ToolDyn>> {
        if !self.includes(name) {
            return None;
        }
        self.registry.create(ctx, name)
    }

    /// Register a pre-built tool (e.g. an MCP tool that already implements
    /// `ToolDyn`). The tool is wrapped in an `Arc` so the factory can
    /// return clones.
    pub fn register_tool(&mut self, name: ToolName, tool: Box<dyn rig::tool::ToolDyn>) {
        let shared: Arc<dyn rig::tool::ToolDyn> = Arc::from(tool);
        self.registry.register(
            name.clone(),
            Box::new(move |_ctx: &Arc<ToolContext>| {
                Box::new(ArcToolDyn(shared.clone())) as Box<dyn rig::tool::ToolDyn>
            }),
        );
        self.filter.insert(name, true);
    }

    /// Include a tool in this set.
    pub fn with_tool(mut self, name: impl Into<ToolName>) -> Self {
        self.filter.insert(name.into(), true);
        self
    }

    /// Exclude a tool from this set.
    pub fn without_tool(mut self, name: impl Into<ToolName>) -> Self {
        self.filter.insert(name.into(), false);
        self
    }

    /// Merge `other` on top of `self`. Entries in `other` override
    /// entries in `self`.
    pub fn merge(mut self, other: &ToolSet) -> Self {
        for (name, &value) in &other.filter {
            self.filter.insert(name.clone(), value);
        }
        self
    }

    /// No tools included.
    pub fn no_tools() -> Self {
        ToolSet {
            filter: HashMap::new(),
            registry: ToolRegistry::with_all_builtins(),
        }
    }

    /// Read-only tools only: read, grep, find_files, list_dir.
    pub fn read_only() -> Self {
        let mut filter = HashMap::new();
        filter.insert(ToolName::read(), true);
        filter.insert(ToolName::grep(), true);
        filter.insert(ToolName::find_files(), true);
        filter.insert(ToolName::list_dir(), true);
        ToolSet {
            filter,
            registry: ToolRegistry::with_all_builtins(),
        }
    }
}

// ── ArcToolDyn wrapper ────────────────────────────────────────────────────────

/// Thin wrapper that delegates `ToolDyn` to an `Arc<dyn ToolDyn>`.
/// This allows pre-built tools to be shared across factory invocations.
struct ArcToolDyn(Arc<dyn rig::tool::ToolDyn>);

impl rig::tool::ToolDyn for ArcToolDyn {
    fn name(&self) -> String {
        self.0.name()
    }

    fn definition(
        &self,
        prompt: String,
    ) -> rig::wasm_compat::WasmBoxedFuture<'_, rig::completion::ToolDefinition> {
        self.0.definition(prompt)
    }

    fn call(
        &self,
        args: String,
    ) -> rig::wasm_compat::WasmBoxedFuture<'_, Result<String, rig::tool::ToolError>> {
        self.0.call(args)
    }
}

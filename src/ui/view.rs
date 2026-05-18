//! First-class view system for the TUI.
//!
//! A "view" is a named buffer that can be displayed in the renderer. The lead
//! session is always the `Lead` view. Any other subsystem (subagents, future
//! summary panels, diff views, etc.) can register a named view by choosing a
//! `ViewId` and writing `LineEntry` buffers into it.
//!
//! # Design
//!
//! Views are a UI-layer concern. This module has no dependency on subagents,
//! teams, or any feature-gated subsystem. Subsystems register views by name;
//! the view system stores and restores their buffers without knowing what kind
//! of thing produced them.
//!
//! # INVARIANT
//!
//! `ViewManager` is owned exclusively by the UI event loop. All view switching
//! runs in that loop. No locks are needed.

use std::collections::HashMap;

use crate::ui::renderer::{LineEntry, Renderer};

/// A string identifier for a named view.
///
/// Named views are identified by human-readable strings (typically the name of
/// the subagent or panel that owns the view). ViewId is used as the key in the
/// view buffer map and displayed in the status bar.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ViewId(pub String);

impl ViewId {
    pub fn new(name: impl Into<String>) -> Self {
        ViewId(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ViewId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for ViewId {
    fn from(s: &str) -> Self {
        ViewId(s.to_string())
    }
}

impl From<String> for ViewId {
    fn from(s: String) -> Self {
        ViewId(s)
    }
}

/// Which view is currently displayed in the renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveView {
    /// The lead agent's session (default).
    Lead,
    /// A named view registered by a subsystem.
    Named(ViewId),
}

/// Manages named view buffers and renderer hot-swapping.
///
/// Holds the lead buffer (saved when a named view is active), all named view
/// buffers, and the currently active view. All methods are synchronous and
/// called only from the UI event loop.
pub struct ViewManager {
    /// Which view is currently rendered.
    pub active: ActiveView,
    /// Lead session buffer, stored while a named view is displayed.
    lead_buffer: Vec<LineEntry>,
    /// Per-view buffers, keyed by ViewId.
    named_buffers: HashMap<ViewId, Vec<LineEntry>>,
}

impl ViewManager {
    pub fn new() -> Self {
        ViewManager {
            active: ActiveView::Lead,
            lead_buffer: Vec::new(),
            named_buffers: HashMap::new(),
        }
    }

    /// Returns `true` if currently showing the lead view.
    pub fn is_lead(&self) -> bool {
        self.active == ActiveView::Lead
    }

    /// Returns the active `ViewId` if in a named view.
    pub fn active_named(&self) -> Option<&ViewId> {
        match &self.active {
            ActiveView::Named(id) => Some(id),
            ActiveView::Lead => None,
        }
    }

    /// Returns the display string for the status bar, or `None` in lead view.
    pub fn status_label(&self) -> Option<String> {
        match &self.active {
            ActiveView::Named(id) => Some(id.as_str().to_string()),
            ActiveView::Lead => None,
        }
    }

    /// Switch to a named view, saving the current buffer first.
    ///
    /// If already in a named view, the current named buffer is saved before
    /// switching. Returns the `ViewId` of the view switched to.
    pub fn switch_to(&mut self, id: ViewId, renderer: &mut Renderer) -> std::io::Result<()> {
        // Save current buffer.
        let current = renderer.take_buffer();
        match &self.active {
            ActiveView::Lead => {
                self.lead_buffer = current;
            }
            ActiveView::Named(prev_id) => {
                self.named_buffers.insert(prev_id.clone(), current);
            }
        }
        // Restore target buffer (empty if first visit).
        let target = self.named_buffers.entry(id.clone()).or_default().clone();
        renderer.restore_buffer(target)?;
        self.active = ActiveView::Named(id);
        Ok(())
    }

    /// Switch back to the lead view, saving the current named buffer.
    ///
    /// No-op if already in lead view.
    pub fn switch_to_lead(&mut self, renderer: &mut Renderer) -> std::io::Result<()> {
        if self.is_lead() {
            return Ok(());
        }
        // Save current named buffer.
        let current = renderer.take_buffer();
        if let ActiveView::Named(id) = &self.active {
            self.named_buffers.insert(id.clone(), current);
        }
        // Restore lead buffer.
        let lead = std::mem::take(&mut self.lead_buffer);
        renderer.restore_buffer(lead)?;
        self.active = ActiveView::Lead;
        Ok(())
    }

    /// Write a buffer snapshot into a named view without switching to it.
    ///
    /// Used by background producers (e.g. subagent relay) to accumulate output
    /// into a view that is not currently displayed. If the view is currently
    /// active, this is a no-op — the renderer already holds the live buffer.
    #[cfg(feature = "subagent")]
    pub fn write_to_inactive(&mut self, id: &ViewId, entries: Vec<LineEntry>) {
        if self.active == ActiveView::Named(id.clone()) {
            // View is currently displayed; renderer holds the live buffer.
            return;
        }
        self.named_buffers
            .entry(id.clone())
            .or_default()
            .extend(entries);
    }

    /// Remove a named view's buffer (e.g. when a subagent is stopped).
    ///
    /// If the view is currently active, switches to lead first.
    #[cfg(feature = "subagent")]
    pub fn remove(&mut self, id: &ViewId, renderer: &mut Renderer) -> std::io::Result<()> {
        if self.active == ActiveView::Named(id.clone()) {
            self.switch_to_lead(renderer)?;
        }
        self.named_buffers.remove(id);
        Ok(())
    }

    /// Returns `true` if a named view with this id has been registered.
    pub fn has_view(&self, id: &ViewId) -> bool {
        self.named_buffers.contains_key(id)
    }

    /// List all registered named view ids.
    pub fn named_views(&self) -> impl Iterator<Item = &ViewId> {
        self.named_buffers.keys()
    }
}

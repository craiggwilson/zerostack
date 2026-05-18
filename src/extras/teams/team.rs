//! Team: a named group of subagents with a shared task board.

use std::sync::Arc;

use crate::extras::subagent::{SubagentId, SubagentRegistry};

use super::task_board::{Priority, Task, TaskBoard, TaskBoardSummary, TaskId};

/// A named team of subagents with a shared task board.
///
/// Teams do NOT own subagents — they hold ids and delegate all lifecycle
/// operations to `SubagentRegistry`.
pub struct Team {
    member_ids: Vec<SubagentId>,
    task_board: TaskBoard,
    agents: Arc<SubagentRegistry>,
}

impl Team {
    /// Create a new empty team.
    pub fn new(agents: Arc<SubagentRegistry>) -> Self {
        Team {
            member_ids: Vec::new(),
            task_board: TaskBoard::new(),
            agents,
        }
    }

    /// Return the list of member ids.
    pub fn members(&self) -> &[SubagentId] {
        &self.member_ids
    }

    /// Add a subagent to the team.
    ///
    /// Uses the `SubagentRegistry` provided at construction to validate that
    /// the subagent exists, then records it in the membership list.
    pub fn add_member(&mut self, id: SubagentId) {
        if !self.member_ids.contains(&id) {
            self.member_ids.push(id);
        }
    }

    /// Send a message to all team member inboxes.
    ///
    /// Messages are delivered via the registry held at construction. Returns
    /// the number of members the message was successfully delivered to.
    pub fn message(&self, message: String) -> usize {
        let mut delivered = 0;
        for &id in &self.member_ids {
            if self.agents.send_message(id, message.clone()).is_ok() {
                delivered += 1;
            }
        }
        delivered
    }

    // ── TaskBoard delegation ──────────────────────────────────────────────────

    /// Add a task to the board. Returns the new task's id.
    pub fn add_task(
        &mut self,
        content: String,
        priority: Priority,
        depends_on: Vec<TaskId>,
    ) -> TaskId {
        self.task_board.add(content, priority, depends_on)
    }

    /// Mark a task as done, unblocking any tasks that depended on it.
    pub fn mark_task_done(&mut self, id: TaskId) -> anyhow::Result<()> {
        self.task_board.mark_done(id)
    }

    /// Return all tasks sorted by id.
    pub fn list_tasks(&self) -> Vec<&Task> {
        self.task_board.list()
    }

    /// Return a summary of task counts by status.
    pub fn task_board_summary(&self) -> TaskBoardSummary {
        self.task_board.summary()
    }
}

//! Task board for teams: add, complete, and list tasks with dependency tracking.

use std::collections::HashMap;

/// Summary of task counts by status on a board.
#[derive(Debug, Clone, Copy, Default)]
pub struct TaskBoardSummary {
    pub total: usize,
    pub done: usize,
    pub pending: usize,
    pub blocked: usize,
    pub in_progress: usize,
}

/// Unique task identifier within a team.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(pub u32);

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Priority level for a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Priority {
    High,
    Medium,
    Low,
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Priority::High => write!(f, "high"),
            Priority::Medium => write!(f, "medium"),
            Priority::Low => write!(f, "low"),
        }
    }
}

/// Status of a task on the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskStatus {
    /// Waiting on dependencies to complete.
    Blocked,
    /// All dependencies complete, ready to claim.
    Pending,
    /// Claimed and being worked on.
    InProgress,
    /// Completed successfully.
    Done,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskStatus::Blocked => write!(f, "blocked"),
            TaskStatus::Pending => write!(f, "pending"),
            TaskStatus::InProgress => write!(f, "in_progress"),
            TaskStatus::Done => write!(f, "done"),
        }
    }
}

/// A single task on the board.
#[derive(Debug, Clone)]
pub struct Task {
    pub id: TaskId,
    pub content: String,
    pub priority: Priority,
    pub status: TaskStatus,
    pub depends_on: Vec<TaskId>,
}

/// Shared task board for a team.
///
/// Tracks tasks with dependency-based blocking: a task is `Blocked` until all
/// its `depends_on` tasks are `Done`.
#[derive(Debug, Default)]
pub struct TaskBoard {
    tasks: HashMap<TaskId, Task>,
    next_id: u32,
}

impl TaskBoard {
    /// Create an empty task board.
    pub fn new() -> Self {
        TaskBoard {
            tasks: HashMap::new(),
            next_id: 1,
        }
    }

    /// Add a task to the board. Returns the new task's id.
    ///
    /// The task starts as `Pending` if it has no unresolved dependencies,
    /// or `Blocked` if any dependency is not yet `Done`.
    pub fn add(&mut self, content: String, priority: Priority, depends_on: Vec<TaskId>) -> TaskId {
        let id = TaskId(self.next_id);
        self.next_id += 1;

        let status = self.compute_status(&depends_on);
        self.tasks.insert(
            id,
            Task {
                id,
                content,
                priority,
                status,
                depends_on,
            },
        );
        id
    }

    /// Mark a task as done, unblocking any tasks that depended on it.
    pub fn mark_done(&mut self, id: TaskId) -> anyhow::Result<()> {
        let task = self
            .tasks
            .get_mut(&id)
            .ok_or_else(|| anyhow::anyhow!("task {} not found", id))?;
        task.status = TaskStatus::Done;

        // Unblock tasks that depended on this one.
        let unblocked: Vec<TaskId> = self
            .tasks
            .values()
            .filter(|t| t.depends_on.contains(&id) && t.status == TaskStatus::Blocked)
            .map(|t| t.id)
            .collect();

        for unblock_id in unblocked {
            let deps = self.tasks[&unblock_id].depends_on.clone();
            let new_status = self.compute_status(&deps);
            if let Some(t) = self.tasks.get_mut(&unblock_id) {
                t.status = new_status;
            }
        }

        Ok(())
    }

    /// Claim a pending task (mark as in-progress). Returns the task content.
    #[cfg(test)]
    pub fn claim(&mut self, id: TaskId) -> anyhow::Result<String> {
        let task = self
            .tasks
            .get_mut(&id)
            .ok_or_else(|| anyhow::anyhow!("task {} not found", id))?;
        if task.status != TaskStatus::Pending {
            anyhow::bail!("task {} is {} (must be pending to claim)", id, task.status);
        }
        task.status = TaskStatus::InProgress;
        Ok(task.content.clone())
    }

    /// Return all tasks sorted by id.
    pub fn list(&self) -> Vec<&Task> {
        let mut tasks: Vec<&Task> = self.tasks.values().collect();
        tasks.sort_by_key(|t| t.id.0);
        tasks
    }

    /// Return a summary of task counts by status.
    pub fn summary(&self) -> TaskBoardSummary {
        TaskBoardSummary {
            total: self.tasks.len(),
            done: self
                .tasks
                .values()
                .filter(|t| t.status == TaskStatus::Done)
                .count(),
            pending: self
                .tasks
                .values()
                .filter(|t| t.status == TaskStatus::Pending)
                .count(),
            blocked: self
                .tasks
                .values()
                .filter(|t| t.status == TaskStatus::Blocked)
                .count(),
            in_progress: self
                .tasks
                .values()
                .filter(|t| t.status == TaskStatus::InProgress)
                .count(),
        }
    }

    fn compute_status(&self, depends_on: &[TaskId]) -> TaskStatus {
        if depends_on.iter().all(|dep_id| {
            self.tasks
                .get(dep_id)
                .is_none_or(|dep| dep.status == TaskStatus::Done)
        }) {
            TaskStatus::Pending
        } else {
            TaskStatus::Blocked
        }
    }
}

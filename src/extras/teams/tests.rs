//! Unit tests for teams subsystem: TaskBoard, Team.

#[cfg(test)]
mod task_board_tests {
    use crate::extras::teams::task_board::{Priority, TaskBoard, TaskId, TaskStatus};

    /// Tasks with no dependencies start as Pending.
    #[test]
    fn add_no_deps_is_pending() {
        let mut board = TaskBoard::new();
        let id = board.add("do something".to_string(), Priority::Medium, vec![]);
        let tasks = board.list();
        let task = tasks.iter().find(|t| t.id == id).unwrap();
        assert_eq!(task.status, TaskStatus::Pending);
    }

    /// Tasks whose dependencies are not done start as Blocked.
    #[test]
    fn add_with_unmet_deps_is_blocked() {
        let mut board = TaskBoard::new();
        let dep_id = board.add("dep".to_string(), Priority::Low, vec![]);
        let blocked_id = board.add("blocked".to_string(), Priority::High, vec![dep_id]);
        let tasks = board.list();
        let blocked = tasks.iter().find(|t| t.id == blocked_id).unwrap();
        assert_eq!(blocked.status, TaskStatus::Blocked);
    }

    /// Completing a task unblocks dependent tasks.
    #[test]
    fn done_unblocks_dependents() {
        let mut board = TaskBoard::new();
        let dep_id = board.add("dep".to_string(), Priority::Low, vec![]);
        let blocked_id = board.add("child".to_string(), Priority::Medium, vec![dep_id]);

        board.mark_done(dep_id).unwrap();

        let tasks = board.list();
        let child = tasks.iter().find(|t| t.id == blocked_id).unwrap();
        assert_eq!(child.status, TaskStatus::Pending);
    }

    /// Claiming a task marks it in_progress.
    #[test]
    fn claim_pending_task() {
        let mut board = TaskBoard::new();
        let id = board.add("work".to_string(), Priority::High, vec![]);
        let content = board.claim(id).unwrap();
        assert_eq!(content, "work");
        let tasks = board.list();
        let task = tasks.iter().find(|t| t.id == id).unwrap();
        assert_eq!(task.status, TaskStatus::InProgress);
    }

    /// Claiming a blocked task fails.
    #[test]
    fn claim_blocked_task_fails() {
        let mut board = TaskBoard::new();
        let dep = board.add("dep".to_string(), Priority::Low, vec![]);
        let id = board.add("child".to_string(), Priority::Low, vec![dep]);
        assert!(board.claim(id).is_err());
    }

    /// Summary returns correct counts.
    #[test]
    fn summary_counts() {
        let mut board = TaskBoard::new();
        let dep = board.add("dep".to_string(), Priority::Low, vec![]);
        let _blocked = board.add("child".to_string(), Priority::Low, vec![dep]);
        board.mark_done(dep).unwrap();
        let summary = board.summary();
        assert_eq!(summary.total, 2);
        assert_eq!(summary.done, 1);
        assert_eq!(summary.pending, 1); // child was unblocked
        assert_eq!(summary.blocked, 0);
        assert_eq!(summary.in_progress, 0);
    }

    /// Marking a nonexistent task returns an error.
    #[test]
    fn done_nonexistent_fails() {
        let mut board = TaskBoard::new();
        assert!(board.mark_done(TaskId(999)).is_err());
    }
}

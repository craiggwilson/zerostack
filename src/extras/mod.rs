#[cfg(feature = "loop")]
pub mod r#loop;

#[cfg(feature = "git-worktree")]
pub mod git_worktree;

#[cfg(feature = "mcp")]
pub mod mcp;

#[cfg(feature = "acp")]
pub mod acp;

#[cfg(feature = "subagent")]
pub mod subagent;

#[cfg(feature = "teams")]
pub mod teams;

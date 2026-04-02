use serde::{Deserialize, Serialize};

use crate::{SessionName, TerminalSize};

/// Request payload for `new-session`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewSessionRequest {
    /// The exact session name to create.
    pub session_name: SessionName,
    /// Whether the session should remain detached after creation.
    pub detached: bool,
    /// The initial pane geometry, when explicitly requested.
    pub size: Option<TerminalSize>,
    /// Optional per-spawn environment overrides in `NAME=VALUE` form.
    #[serde(default)]
    pub environment: Option<Vec<String>>,
}

/// Extended request payload for `new-session`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewSessionExtRequest {
    /// The optional exact session name to create.
    pub session_name: Option<SessionName>,
    /// Optional tmux format-expanded start directory for the new session.
    #[serde(default)]
    pub working_directory: Option<String>,
    /// Whether the session should remain detached after creation.
    pub detached: bool,
    /// The initial pane geometry, when explicitly requested.
    pub size: Option<TerminalSize>,
    /// Optional per-spawn environment overrides in `NAME=VALUE` form.
    #[serde(default)]
    pub environment: Option<Vec<String>>,
    /// The optional target session or group name for grouped-session creation.
    #[serde(default)]
    pub group_target: Option<SessionName>,
    /// Whether an existing target session should be attached instead of erroring.
    #[serde(default)]
    pub attach_if_exists: bool,
    /// Whether other attached clients should be detached before attaching.
    #[serde(default)]
    pub detach_other_clients: bool,
    /// Whether other attached clients should be detached and terminated.
    #[serde(default)]
    pub kill_other_clients: bool,
    /// Optional tmux client-flag names such as `read-only` or `active-pane`.
    #[serde(default)]
    pub flags: Option<Vec<String>>,
    /// The optional initial active-window name for standalone session creation.
    #[serde(default)]
    pub window_name: Option<String>,
    /// Whether the created session should print formatted session information.
    #[serde(default)]
    pub print_session_info: bool,
    /// The optional format template used when printing session information.
    #[serde(default)]
    pub print_format: Option<String>,
    /// Optional shell command argv. A single argument is executed via `$SHELL -c`.
    #[serde(default)]
    pub command: Option<Vec<String>>,
}

/// Request payload for `has-session`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HasSessionRequest {
    /// The exact target session name.
    pub target: SessionName,
}

/// Request payload for `kill-session`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KillSessionRequest {
    /// The exact target session name.
    pub target: SessionName,
    /// Whether every other session should be destroyed instead of the target session.
    #[serde(default)]
    pub kill_all_except_target: bool,
    /// Whether the target session's window alert flags should be cleared instead of destroying it.
    #[serde(default)]
    pub clear_alerts: bool,
}

/// Request payload for `rename-session`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameSessionRequest {
    /// The exact existing session name.
    pub target: SessionName,
    /// The validated destination session name.
    pub new_name: SessionName,
}

/// Request payload for `list-sessions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListSessionsRequest {
    /// An optional server-side format template.
    pub format: Option<String>,
    /// An optional server-side filter expression.
    #[serde(default)]
    pub filter: Option<String>,
    /// The optional tmux sort order name.
    #[serde(default)]
    pub sort_order: Option<String>,
    /// Whether the selected sort order should be reversed.
    #[serde(default)]
    pub reversed: bool,
}

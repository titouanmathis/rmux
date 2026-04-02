use serde::{Deserialize, Serialize};

use super::CommandOutput;
use crate::SessionName;

/// Response payload for `kill-server`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KillServerResponse;

/// Response payload for `lock-server`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockServerResponse;

/// Response payload for `lock-session`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockSessionResponse {
    /// The session whose attached clients were considered for locking.
    pub target: SessionName,
}

/// Response payload for `lock-client`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockClientResponse {
    /// The client identifier that was targeted.
    pub target_client: String,
}

/// Response payload for `server-access`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerAccessResponse {
    /// Expanded stdout for `server-access`.
    pub output: CommandOutput,
}

impl ServerAccessResponse {
    /// Builds a response with no stdout payload.
    #[must_use]
    pub const fn no_output(output: CommandOutput) -> Self {
        Self { output }
    }

    /// Returns the stdout payload for `server-access`.
    #[must_use]
    pub const fn command_output(&self) -> &CommandOutput {
        &self.output
    }
}

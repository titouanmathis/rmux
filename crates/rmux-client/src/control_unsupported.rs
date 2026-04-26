//! Unsupported control-mode surface for Windows builds.

use std::io::{self, Read, Write};

use rmux_proto::{ClientTerminalContext, ControlMode};

use crate::{
    connection::{Connection, ControlModeUpgrade, ControlTransition},
    ClientError,
};

impl Connection {
    /// Requests a control-mode upgrade.
    pub fn begin_control_mode(
        self,
        _mode: ControlMode,
        _client_terminal: ClientTerminalContext,
    ) -> Result<ControlTransition, ClientError> {
        let _ = self;
        Err(ClientError::Io(io::Error::new(
            io::ErrorKind::Unsupported,
            "control mode is not enabled on Windows yet",
        )))
    }
}

/// Drives a control-mode session using the process stdio streams.
pub fn drive_control_mode(
    _upgrade: ControlModeUpgrade,
    _initial_commands: &[String],
) -> Result<(), ClientError> {
    Err(ClientError::Io(io::Error::new(
        io::ErrorKind::Unsupported,
        "control mode is not enabled on Windows yet",
    )))
}

/// Drives a control-mode session using explicit input and output streams.
pub fn drive_control_mode_with_stdio<R, W>(
    _upgrade: ControlModeUpgrade,
    _initial_commands: &[String],
    _input: R,
    _output: W,
) -> Result<(), ClientError>
where
    R: Read + Send + 'static,
    W: Write,
{
    Err(ClientError::Io(io::Error::new(
        io::ErrorKind::Unsupported,
        "control mode is not enabled on Windows yet",
    )))
}

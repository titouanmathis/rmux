use super::{after_hook_format_values, RequestHandler, DEFAULT_SESSION_SIZE};
use crate::control::ControlModeUpgrade;
use crate::daemon::ShutdownHandle;
use crate::pane_io::AttachControl;
use rmux_core::{
    command_parser::parse_command_string, PaneGeometry, WINDOW_ALERTFLAGS, WINLINK_ALERTFLAGS,
};
use rmux_proto::{
    ControlMode, ErrorResponse, HasSessionRequest, HookName, KillPaneRequest, KillSessionRequest,
    LayoutName, ListPanesRequest, ListSessionsRequest, NewSessionExtRequest, NewSessionRequest,
    OptionName, PaneTarget, RenameSessionRequest, Request, ResizePaneAdjustment, Response,
    RmuxError, ScopeSelector, SelectPaneRequest, SessionName, SetOptionMode, SetOptionRequest,
    SplitWindowRequest, SplitWindowTarget, TerminalSize,
};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

fn session_name(value: &str) -> SessionName {
    SessionName::new(value).expect("valid session name")
}

#[path = "handler_tests/new_session.rs"]
mod new_session;

#[path = "handler_tests/kill_session.rs"]
mod kill_session;

#[path = "handler_tests/clients.rs"]
mod clients;

#[path = "handler_tests/panes.rs"]
mod panes;

#[path = "handler_tests/rename_session.rs"]
mod rename_session;

#[path = "handler_tests/lists_and_hooks.rs"]
mod lists_and_hooks;

#[path = "handler_send_keys_tests.rs"]
mod send_keys_tests;

#[path = "handler_copy_mode_tests.rs"]
mod copy_mode_tests;

#[path = "handler_overlay_tests.rs"]
mod overlay_tests;

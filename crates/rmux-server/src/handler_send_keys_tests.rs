use super::super::RequestHandler;
use super::session_name;
use crate::copy_mode::CopyPosition;
use crate::input_keys::{encode_key, encode_mouse_event, ExtendedKeyFormat, MouseForwardEvent};
use crate::mouse::{AttachedMouseEvent, MouseLocation};
use rmux_core::{input::mode, key_string_lookup_string};
use rmux_proto::{
    BindKeyRequest, CopyModeRequest, ErrorResponse, ListKeysRequest, ListPanesRequest,
    NewSessionRequest, OptionName, PaneTarget, Request, Response, RmuxError, ScopeSelector,
    SendKeysExtRequest, SendKeysRequest, SendKeysResponse, SendPrefixRequest, SetOptionMode,
    SetOptionRequest, ShowBufferRequest, SwitchClientExtRequest, TerminalSize, UnbindKeyRequest,
    WindowTarget,
};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;

static UNIQUE_ID: AtomicUsize = AtomicUsize::new(0);

fn unique_output_path(label: &str) -> PathBuf {
    let unique_id = UNIQUE_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "rmux-{label}-{}-{unique_id}.bin",
        std::process::id()
    ));
    let _ = fs::remove_file(&path);
    path
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

async fn start_cat_capture(
    handler: &RequestHandler,
    session_name: &rmux_proto::SessionName,
    path: &Path,
) {
    let response = handler
        .handle(Request::SendKeys(SendKeysRequest {
            target: PaneTarget::new(session_name.clone(), 0),
            keys: vec![format!("cat > {}", shell_quote(path)), "Enter".to_owned()],
        }))
        .await;
    assert_eq!(
        response,
        Response::SendKeys(SendKeysResponse { key_count: 2 })
    );
    wait_for_file_to_exist(path)
        .await
        .expect("cat capture file should be created by shell redirection");
}

async fn finish_cat_capture(handler: &RequestHandler, session_name: &rmux_proto::SessionName) {
    let response = handler
        .handle(Request::SendKeysExt(SendKeysExtRequest {
            target: Some(PaneTarget::new(session_name.clone(), 0)),
            keys: vec!["04".to_owned()],
            expand_formats: false,
            hex: true,
            literal: false,
            dispatch_key_table: false,
            copy_mode_command: false,
            forward_mouse_event: false,
            reset_terminal: false,
            repeat_count: None,
        }))
        .await;
    assert_eq!(
        response,
        Response::SendKeys(SendKeysResponse { key_count: 1 })
    );
}

async fn wait_for_file_bytes(path: &Path, expected: &[u8]) -> Result<(), io::Error> {
    for _ in 0..100 {
        match fs::read(path) {
            Ok(contents) if contents == expected => return Ok(()),
            Ok(_) | Err(_) => sleep(Duration::from_millis(20)).await,
        }
    }

    Err(io::Error::other(format!(
        "file '{}' never reached expected contents",
        path.display()
    )))
}

async fn wait_for_file_to_exist(path: &Path) -> Result<(), io::Error> {
    for _ in 0..100 {
        if path.exists() {
            return Ok(());
        }
        sleep(Duration::from_millis(20)).await;
    }

    Err(io::Error::other(format!(
        "file '{}' was not created",
        path.display()
    )))
}

#[path = "handler_send_keys_tests/basic_dispatch.rs"]
mod basic_dispatch;

#[path = "handler_send_keys_tests/bindings_timeouts.rs"]
mod bindings_timeouts;

#[path = "handler_send_keys_tests/live_attach.rs"]
mod live_attach;

#[path = "handler_send_keys_tests/mouse_copy_mode.rs"]
mod mouse_copy_mode;

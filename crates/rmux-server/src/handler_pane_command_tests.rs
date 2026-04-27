use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::RequestHandler;
use crate::pane_io::AttachControl;
use rmux_proto::{
    BreakPaneRequest, DisplayPanesRequest, MovePaneRequest, NewSessionRequest, OptionName,
    PaneTarget, PipePaneRequest, RenameWindowRequest, Request, RespawnPaneRequest, ScopeSelector,
    SelectPaneRequest, SendKeysRequest, SessionName, SetOptionMode, SetOptionRequest,
    SplitDirection, SplitWindowRequest, SplitWindowTarget, TerminalSize, WindowTarget,
};
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout};

fn session_name(value: &str) -> SessionName {
    SessionName::new(value).expect("valid session name")
}

fn unique_temp_path(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "rmux-pane-command-{label}-{}-{unique}",
        std::process::id()
    ))
}

#[cfg(unix)]
fn shell_quote(path: &Path) -> String {
    crate::test_shell::sh_quote_path(path)
}

#[cfg(windows)]
fn pipe_to_file_command(path: &Path) -> String {
    crate::test_shell::powershell_encoded_command(&format!(
        "$out=[System.IO.File]::Open({}, [System.IO.FileMode]::Create, [System.IO.FileAccess]::Write, [System.IO.FileShare]::ReadWrite); try {{ $buf=New-Object byte[] 4096; $inputStream=[Console]::OpenStandardInput(); while (($n=$inputStream.Read($buf,0,$buf.Length)) -gt 0) {{ $out.Write($buf,0,$n); $out.Flush() }} }} finally {{ $out.Dispose() }}",
        crate::test_shell::powershell_quote_path(path)
    ))
}

#[cfg(unix)]
fn pipe_to_file_command(path: &Path) -> String {
    format!("cat > {}", shell_quote(path))
}

fn pipe_discard_command() -> String {
    crate::test_shell::stdin_discard_command()
}

#[cfg(unix)]
fn pane_print_command(text: &str) -> String {
    format!("printf '{}\\n'", text.replace('\'', r"'\''"))
}

#[cfg(windows)]
fn pane_print_command(text: &str) -> String {
    format!("echo {text}")
}

#[cfg(unix)]
fn respawn_probe_command(output: &Path) -> String {
    format!(
        "printf '%s:%s' \"$(pwd)\" \"$RMUX_RESPAWN\" > {}",
        shell_quote(output)
    )
}

#[cfg(windows)]
fn respawn_probe_command(output: &Path) -> String {
    crate::test_shell::powershell_encoded_command(&format!(
        "[System.IO.File]::WriteAllText({}, ((Get-Location).Path + ':' + $env:RMUX_RESPAWN))",
        crate::test_shell::powershell_quote_path(output)
    ))
}

async fn create_session(handler: &RequestHandler, session_name: &SessionName) {
    let created = handler
        .handle(Request::NewSession(NewSessionRequest {
            session_name: session_name.clone(),
            detached: true,
            size: Some(TerminalSize { cols: 80, rows: 24 }),
            environment: None,
        }))
        .await;
    assert!(matches!(created, rmux_proto::Response::NewSession(_)));
}

async fn wait_for_file_contents(path: &Path, expected: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        match fs::read_to_string(path) {
            Ok(contents) if contents == expected => return,
            Ok(_) | Err(_) if tokio::time::Instant::now() < deadline => {
                sleep(Duration::from_millis(25)).await;
            }
            Ok(contents) => panic!(
                "timed out waiting for {} to contain {:?}, got {:?}",
                path.display(),
                expected,
                contents
            ),
            Err(error) => panic!(
                "timed out waiting for {} to exist with {:?}: {error}",
                path.display(),
                expected
            ),
        }
    }
}

async fn wait_for_file_contains(path: &Path, expected: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        match fs::read_to_string(path) {
            Ok(contents) if contents.contains(expected) => return,
            Ok(_) | Err(_) if tokio::time::Instant::now() < deadline => {
                sleep(Duration::from_millis(25)).await;
            }
            Ok(contents) => panic!(
                "timed out waiting for {} to contain {:?}, got {:?}",
                path.display(),
                expected,
                contents
            ),
            Err(error) => panic!(
                "timed out waiting for {} to exist containing {:?}: {error}",
                path.display(),
                expected
            ),
        }
    }
}

async fn wait_for_dead_pane(
    handler: &RequestHandler,
    session_name: &SessionName,
    window_index: u32,
    pane_index: u32,
) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let exited = {
            let mut state = handler.state.lock().await;
            state
                .clone_pane_master_if_alive(session_name, window_index, pane_index)
                .is_err()
        };
        if exited {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for pane {session_name}:{window_index}.{pane_index} to exit"
        );
        sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn move_pane_routes_through_join_semantics() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    create_session(&handler, &alpha).await;

    assert!(matches!(
        handler
            .handle(Request::SplitWindow(SplitWindowRequest {
                target: SplitWindowTarget::Session(alpha.clone()),
                direction: SplitDirection::Vertical,
                environment: None,
            }))
            .await,
        rmux_proto::Response::SplitWindow(_)
    ));
    {
        let mut state = handler.state.lock().await;
        let pane_id = state.sessions.allocate_pane_id();
        state
            .sessions
            .session_mut(&alpha)
            .expect("session exists")
            .insert_window_with_initial_pane_with_id(
                1,
                TerminalSize { cols: 80, rows: 24 },
                pane_id,
            )
            .expect("window insert succeeds");
        state
            .insert_window_terminal(
                &alpha,
                1,
                crate::pane_terminals::WindowSpawnOptions {
                    start_directory: None,
                    command: None,
                    socket_path: Path::new("/tmp/rmux-test.sock"),
                    environment_overrides: None,
                    pane_alert_callback: None,
                    pane_exit_callback: None,
                },
            )
            .expect("window terminal insert succeeds");
    }

    let response = handler
        .handle(Request::MovePane(MovePaneRequest {
            source: PaneTarget::with_window(alpha.clone(), 0, 1),
            target: PaneTarget::with_window(alpha.clone(), 1, 0),
            direction: SplitDirection::Vertical,
            detached: true,
            before: true,
            full_size: false,
            size: Some(rmux_proto::PaneSplitSize::Absolute(12)),
        }))
        .await;

    assert_eq!(
        response,
        rmux_proto::Response::MovePane(rmux_proto::MovePaneResponse {
            target: PaneTarget::with_window(alpha.clone(), 1, 0),
        })
    );

    let state = handler.state.lock().await;
    let session = state.sessions.session(&alpha).expect("session exists");
    assert_eq!(
        session
            .window_at(1)
            .expect("destination window exists")
            .panes()
            .iter()
            .map(|pane| pane.index())
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
}

#[tokio::test]
async fn break_pane_print_target_uses_custom_format() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    create_session(&handler, &alpha).await;

    assert!(matches!(
        handler
            .handle(Request::SplitWindow(SplitWindowRequest {
                target: SplitWindowTarget::Session(alpha.clone()),
                direction: SplitDirection::Vertical,
                environment: None,
            }))
            .await,
        rmux_proto::Response::SplitWindow(_)
    ));

    let response = handler
        .handle(Request::BreakPane(BreakPaneRequest {
            source: PaneTarget::with_window(alpha.clone(), 0, 1),
            target: Some(WindowTarget::with_window(alpha.clone(), 1)),
            name: None,
            detached: true,
            after: false,
            before: false,
            print_target: true,
            format: Some("#{window_index}.#{pane_index}".to_owned()),
        }))
        .await;

    let rmux_proto::Response::BreakPane(success) = response else {
        panic!("expected break-pane response");
    };
    let output = success.command_output().expect("break-pane -P output");
    assert_eq!(output.stdout(), b"1.0\n");
}

#[tokio::test]
async fn pipe_pane_once_keeps_the_existing_pipe() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    let first_output = unique_temp_path("pipe-once-first");
    let second_output = unique_temp_path("pipe-once-second");
    create_session(&handler, &alpha).await;

    let first = handler
        .handle(Request::PipePane(PipePaneRequest {
            target: PaneTarget::with_window(alpha.clone(), 0, 0),
            stdin: false,
            stdout: true,
            once: false,
            command: Some(pipe_to_file_command(&first_output)),
        }))
        .await;
    assert!(matches!(first, rmux_proto::Response::PipePane(_)));

    let second = handler
        .handle(Request::PipePane(PipePaneRequest {
            target: PaneTarget::with_window(alpha.clone(), 0, 0),
            stdin: false,
            stdout: true,
            once: true,
            command: Some(pipe_to_file_command(&second_output)),
        }))
        .await;
    assert!(matches!(second, rmux_proto::Response::PipePane(_)));

    let sent = handler
        .handle(Request::SendKeys(SendKeysRequest {
            target: PaneTarget::with_window(alpha.clone(), 0, 0),
            keys: vec![pane_print_command("pipe-once-test"), "Enter".to_owned()],
        }))
        .await;
    assert!(matches!(sent, rmux_proto::Response::SendKeys(_)));

    wait_for_file_contains(&first_output, "pipe-once-test").await;
    sleep(Duration::from_millis(150)).await;
    assert!(
        !second_output.exists(),
        "toggle-once should not replace the existing pipe"
    );

    let _ = handler
        .handle(Request::PipePane(PipePaneRequest {
            target: PaneTarget::with_window(alpha, 0, 0),
            stdin: false,
            stdout: true,
            once: false,
            command: None,
        }))
        .await;
    let _ = fs::remove_file(first_output);
}

#[tokio::test]
async fn pipe_pane_rejects_dead_panes() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    create_session(&handler, &alpha).await;
    assert!(matches!(
        handler
            .handle(Request::SetOption(SetOptionRequest {
                scope: ScopeSelector::Pane(PaneTarget::with_window(alpha.clone(), 0, 0)),
                option: OptionName::RemainOnExit,
                value: "on".to_owned(),
                mode: SetOptionMode::Replace,
            }))
            .await,
        rmux_proto::Response::SetOption(_)
    ));

    let respawned = handler
        .handle(Request::RespawnPane(RespawnPaneRequest {
            target: PaneTarget::with_window(alpha.clone(), 0, 0),
            kill: true,
            start_directory: None,
            environment: None,
            command: Some(vec!["exit 0".to_owned()]),
        }))
        .await;
    assert!(matches!(respawned, rmux_proto::Response::RespawnPane(_)));
    wait_for_dead_pane(&handler, &alpha, 0, 0).await;

    let response = handler
        .handle(Request::PipePane(PipePaneRequest {
            target: PaneTarget::with_window(alpha, 0, 0),
            stdin: false,
            stdout: true,
            once: false,
            command: Some(pipe_discard_command()),
        }))
        .await;

    assert!(
        matches!(&response, rmux_proto::Response::Error(error) if error.error.to_string().contains("target pane has exited")),
        "expected dead-pane error, got {response:?}"
    );
}

#[tokio::test]
async fn respawn_pane_rejects_active_pane_without_kill_flag() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    create_session(&handler, &alpha).await;

    let response = handler
        .handle(Request::RespawnPane(RespawnPaneRequest {
            target: PaneTarget::with_window(alpha, 0, 0),
            kill: false,
            start_directory: None,
            environment: None,
            command: None,
        }))
        .await;

    assert!(
        matches!(&response, rmux_proto::Response::Error(error) if error.error.to_string().contains("still active")),
        "expected still-active error, got {response:?}"
    );
}

#[tokio::test]
async fn respawn_pane_with_kill_flag_applies_directory_environment_and_command() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    let cwd = unique_temp_path("respawn-pane-cwd");
    let output = unique_temp_path("respawn-pane-output");
    fs::create_dir_all(&cwd).expect("respawn pane cwd");
    create_session(&handler, &alpha).await;

    let response = handler
        .handle(Request::RespawnPane(RespawnPaneRequest {
            target: PaneTarget::with_window(alpha.clone(), 0, 0),
            kill: true,
            start_directory: Some(cwd.clone()),
            environment: Some(vec!["RMUX_RESPAWN=ready".to_owned()]),
            command: Some(vec![respawn_probe_command(&output)]),
        }))
        .await;

    assert_eq!(
        response,
        rmux_proto::Response::RespawnPane(rmux_proto::RespawnPaneResponse {
            target: PaneTarget::with_window(alpha, 0, 0),
        })
    );
    wait_for_file_contents(&output, &format!("{}:ready", cwd.display())).await;
    let _ = fs::remove_file(output);
    let _ = fs::remove_dir_all(cwd);
}

#[tokio::test]
async fn display_panes_uses_the_default_select_pane_template() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    let requester_pid = 42_u32;
    create_session(&handler, &alpha).await;

    assert!(matches!(
        handler
            .handle(Request::SplitWindow(SplitWindowRequest {
                target: SplitWindowTarget::Session(alpha.clone()),
                direction: SplitDirection::Vertical,
                environment: None,
            }))
            .await,
        rmux_proto::Response::SplitWindow(_)
    ));
    assert!(matches!(
        handler
            .handle(Request::SelectPane(SelectPaneRequest {
                target: PaneTarget::with_window(alpha.clone(), 0, 0),
                title: None,
            }))
            .await,
        rmux_proto::Response::SelectPane(_)
    ));

    let (control_tx, mut control_rx) = mpsc::unbounded_channel();
    handler
        .register_attach(requester_pid, alpha.clone(), control_tx)
        .await;

    let response = handler
        .handle(Request::DisplayPanes(DisplayPanesRequest {
            target: alpha.clone(),
            duration_ms: Some(5_000),
            non_blocking: true,
            no_command: false,
            template: None,
        }))
        .await;
    assert!(matches!(response, rmux_proto::Response::DisplayPanes(_)));
    let _overlay = control_rx.recv().await.expect("display-panes overlay");

    handler
        .handle_attached_live_input_for_test(requester_pid, b"1")
        .await
        .expect("display-panes select input");
    let _clear = control_rx
        .recv()
        .await
        .expect("display-panes clear overlay");

    let state = handler.state.lock().await;
    let session = state.sessions.session(&alpha).expect("session exists");
    assert_eq!(session.active_pane_index(), 1);
}

#[tokio::test]
async fn display_panes_without_a_command_keeps_the_active_pane() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    let requester_pid = 43_u32;
    create_session(&handler, &alpha).await;

    assert!(matches!(
        handler
            .handle(Request::SplitWindow(SplitWindowRequest {
                target: SplitWindowTarget::Session(alpha.clone()),
                direction: SplitDirection::Vertical,
                environment: None,
            }))
            .await,
        rmux_proto::Response::SplitWindow(_)
    ));
    assert!(matches!(
        handler
            .handle(Request::SelectPane(SelectPaneRequest {
                target: PaneTarget::with_window(alpha.clone(), 0, 0),
                title: None,
            }))
            .await,
        rmux_proto::Response::SelectPane(_)
    ));

    let (control_tx, mut control_rx) = mpsc::unbounded_channel();
    handler
        .register_attach(requester_pid, alpha.clone(), control_tx)
        .await;

    let response = handler
        .handle(Request::DisplayPanes(DisplayPanesRequest {
            target: alpha.clone(),
            duration_ms: Some(5_000),
            non_blocking: true,
            no_command: true,
            template: None,
        }))
        .await;
    assert!(matches!(response, rmux_proto::Response::DisplayPanes(_)));
    let _overlay = control_rx.recv().await.expect("display-panes overlay");

    handler
        .handle_attached_live_input_for_test(requester_pid, b"1")
        .await
        .expect("display-panes close input");
    let _clear = control_rx
        .recv()
        .await
        .expect("display-panes clear overlay");

    let state = handler.state.lock().await;
    let session = state.sessions.session(&alpha).expect("session exists");
    assert_eq!(session.active_pane_index(), 0);
}

#[tokio::test]
async fn display_panes_uses_the_session_option_duration_by_default() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    let requester_pid = 44_u32;
    create_session(&handler, &alpha).await;

    {
        let mut state = handler.state.lock().await;
        state
            .options
            .set(
                ScopeSelector::Session(alpha.clone()),
                OptionName::DisplayPanesTime,
                "25".to_owned(),
                SetOptionMode::Replace,
            )
            .expect("set display-panes-time");
    }

    let (control_tx, mut control_rx) = mpsc::unbounded_channel();
    handler
        .register_attach(requester_pid, alpha.clone(), control_tx)
        .await;

    let response = handler
        .handle(Request::DisplayPanes(DisplayPanesRequest {
            target: alpha.clone(),
            duration_ms: None,
            non_blocking: true,
            no_command: true,
            template: None,
        }))
        .await;
    assert!(matches!(response, rmux_proto::Response::DisplayPanes(_)));
    let _overlay = control_rx.recv().await.expect("display-panes overlay");

    timeout(Duration::from_millis(250), async {
        loop {
            let cleared = {
                let active_attach = handler.active_attach.lock().await;
                active_attach
                    .by_pid
                    .get(&requester_pid)
                    .and_then(|active| active.display_panes.as_ref())
                    .is_none()
            };
            if cleared {
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("display-panes state should clear with option duration");
}

#[tokio::test]
async fn display_panes_timeout_emits_a_clear_overlay_to_the_attached_client() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    let requester_pid = 45_u32;
    create_session(&handler, &alpha).await;

    let (control_tx, mut control_rx) = mpsc::unbounded_channel();
    handler
        .register_attach(requester_pid, alpha.clone(), control_tx)
        .await;

    let response = handler
        .handle(Request::DisplayPanes(DisplayPanesRequest {
            target: alpha.clone(),
            duration_ms: Some(25),
            non_blocking: true,
            no_command: true,
            template: None,
        }))
        .await;
    assert!(matches!(response, rmux_proto::Response::DisplayPanes(_)));

    let first = timeout(Duration::from_secs(1), control_rx.recv())
        .await
        .expect("overlay should arrive")
        .expect("overlay command");
    assert!(matches!(first, AttachControl::Overlay(_)));

    let mut seen = Vec::new();
    let clear = timeout(Duration::from_secs(1), async {
        loop {
            let next = control_rx.recv().await.expect("follow-up control");
            match next {
                AttachControl::Overlay(clear) => break clear,
                other => seen.push(format!("{other:?}")),
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("clear overlay should arrive; saw {seen:?}"));
    assert!(
        !clear.frame.is_empty(),
        "display-panes clear overlay should repaint the client"
    );
}

#[tokio::test]
async fn join_pane_rejects_same_source_and_target() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    create_session(&handler, &alpha).await;

    let response = handler
        .handle(Request::JoinPane(rmux_proto::JoinPaneRequest {
            source: PaneTarget::with_window(alpha.clone(), 0, 0),
            target: PaneTarget::with_window(alpha.clone(), 0, 0),
            direction: SplitDirection::Vertical,
            detached: false,
            before: false,
            full_size: false,
            size: None,
        }))
        .await;

    assert!(
        matches!(&response, rmux_proto::Response::Error(error) if error.error.to_string().contains("must be different")),
        "expected same-pane error, got {response:?}"
    );
}

#[tokio::test]
async fn move_pane_rejects_same_source_and_target() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    create_session(&handler, &alpha).await;

    let response = handler
        .handle(Request::MovePane(MovePaneRequest {
            source: PaneTarget::with_window(alpha.clone(), 0, 0),
            target: PaneTarget::with_window(alpha.clone(), 0, 0),
            direction: SplitDirection::Vertical,
            detached: false,
            before: false,
            full_size: false,
            size: None,
        }))
        .await;

    assert!(
        matches!(&response, rmux_proto::Response::Error(error) if error.error.to_string().contains("must be different")),
        "expected same-pane error, got {response:?}"
    );
}

#[tokio::test]
async fn swap_pane_self_swap_is_a_no_op() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    create_session(&handler, &alpha).await;

    assert!(matches!(
        handler
            .handle(Request::SplitWindow(SplitWindowRequest {
                target: SplitWindowTarget::Session(alpha.clone()),
                direction: SplitDirection::Vertical,
                environment: None,
            }))
            .await,
        rmux_proto::Response::SplitWindow(_)
    ));

    let response = handler
        .handle(Request::SwapPane(rmux_proto::SwapPaneRequest {
            source: PaneTarget::with_window(alpha.clone(), 0, 0),
            target: PaneTarget::with_window(alpha.clone(), 0, 0),
            direction: None,
            detached: false,
            preserve_zoom: false,
        }))
        .await;

    assert!(
        matches!(response, rmux_proto::Response::SwapPane(_)),
        "self-swap should succeed as a no-op, got {response:?}"
    );
}

#[tokio::test]
async fn respawn_pane_dead_pane_succeeds_without_kill_flag() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    let target = PaneTarget::with_window(alpha.clone(), 0, 0);
    create_session(&handler, &alpha).await;

    assert!(matches!(
        handler
            .handle(Request::SetOption(SetOptionRequest {
                scope: ScopeSelector::Pane(target.clone()),
                option: OptionName::RemainOnExit,
                value: "on".to_owned(),
                mode: SetOptionMode::Replace,
            }))
            .await,
        rmux_proto::Response::SetOption(_)
    ));

    let respawned = handler
        .handle(Request::RespawnPane(RespawnPaneRequest {
            target: target.clone(),
            kill: true,
            start_directory: None,
            environment: None,
            command: Some(vec!["exit 0".to_owned()]),
        }))
        .await;
    assert!(matches!(respawned, rmux_proto::Response::RespawnPane(_)));
    wait_for_dead_pane(&handler, &alpha, 0, 0).await;

    let response = handler
        .handle(Request::RespawnPane(RespawnPaneRequest {
            target,
            kill: false,
            start_directory: None,
            environment: None,
            command: None,
        }))
        .await;

    assert!(
        matches!(response, rmux_proto::Response::RespawnPane(_)),
        "respawning a dead pane without -k should succeed, got {response:?}"
    );
}

#[tokio::test]
async fn remain_on_exit_keeps_the_existing_window_name() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    let target = PaneTarget::with_window(alpha.clone(), 0, 0);
    create_session(&handler, &alpha).await;

    assert!(matches!(
        handler
            .handle(Request::RenameWindow(RenameWindowRequest {
                target: WindowTarget::with_window(alpha.clone(), 0),
                name: "custom".to_owned(),
            }))
            .await,
        rmux_proto::Response::RenameWindow(_)
    ));

    assert!(matches!(
        handler
            .handle(Request::SetOption(SetOptionRequest {
                scope: ScopeSelector::Pane(target.clone()),
                option: OptionName::RemainOnExit,
                value: "on".to_owned(),
                mode: SetOptionMode::Replace,
            }))
            .await,
        rmux_proto::Response::SetOption(_)
    ));

    let expected_window_name = {
        let state = handler.state.lock().await;
        state
            .sessions
            .session(&alpha)
            .and_then(|session| session.window_at(0))
            .and_then(|window| window.name())
            .expect("renamed window keeps its explicit name")
            .to_owned()
    };

    let response = handler
        .handle(Request::RespawnPane(RespawnPaneRequest {
            target: target.clone(),
            kill: true,
            start_directory: None,
            environment: None,
            command: Some(vec!["exit 0".to_owned()]),
        }))
        .await;
    assert!(matches!(response, rmux_proto::Response::RespawnPane(_)));
    wait_for_dead_pane(&handler, &alpha, 0, 0).await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let (ready, observation) = {
            let state = handler.state.lock().await;
            match state
                .sessions
                .session(&alpha)
                .and_then(|session| session.window_at(0))
                .and_then(|window| {
                    window
                        .pane(0)
                        .map(|pane| (window.name().map(str::to_owned), pane.id()))
                }) {
                Some((window_name, pane_id)) => {
                    let dead = state.pane_is_dead(&alpha, pane_id);
                    (
                        window_name.as_deref() == Some(expected_window_name.as_str()) && dead,
                        format!(
                            "last_window_name={window_name:?} last_dead={dead:?} last_pane_id={:?}",
                            pane_id.as_u32()
                        ),
                    )
                }
                None => (
                    false,
                    "last_window_name=None last_dead=None last_pane_id=None".to_owned(),
                ),
            }
        };
        if ready {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "timed out waiting for remain-on-exit window name to stay at {expected_window_name:?}; {observation}"
            );
        }
        sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn remain_on_exit_auto_named_window_gets_tmux_dead_suffix_when_unattached() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    let target = PaneTarget::with_window(alpha.clone(), 0, 0);
    create_session(&handler, &alpha).await;

    assert!(matches!(
        handler
            .handle(Request::SetOption(SetOptionRequest {
                scope: ScopeSelector::Pane(target.clone()),
                option: OptionName::RemainOnExit,
                value: "on".to_owned(),
                mode: SetOptionMode::Replace,
            }))
            .await,
        rmux_proto::Response::SetOption(_)
    ));

    let expected_window_name = "exit[dead]".to_owned();

    let response = handler
        .handle(Request::RespawnPane(RespawnPaneRequest {
            target: target.clone(),
            kill: true,
            start_directory: None,
            environment: None,
            command: Some(vec!["exit 0".to_owned()]),
        }))
        .await;
    assert!(matches!(response, rmux_proto::Response::RespawnPane(_)));
    wait_for_dead_pane(&handler, &alpha, 0, 0).await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let (ready, observation) = {
            let state = handler.state.lock().await;
            match state
                .sessions
                .session(&alpha)
                .and_then(|session| session.window_at(0))
                .and_then(|window| {
                    window
                        .pane(0)
                        .map(|pane| (window.name().map(str::to_owned), pane.id()))
                }) {
                Some((window_name, pane_id)) => {
                    let dead = state.pane_is_dead(&alpha, pane_id);
                    (
                        window_name.as_deref() == Some(expected_window_name.as_str()) && dead,
                        format!(
                            "last_window_name={window_name:?} last_dead={dead:?} last_pane_id={:?}",
                            pane_id.as_u32()
                        ),
                    )
                }
                None => (
                    false,
                    "last_window_name=None last_dead=None last_pane_id=None".to_owned(),
                ),
            }
        };
        if ready {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "timed out waiting for remain-on-exit automatic dead name {expected_window_name:?}; {observation}"
            );
        }
        sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn pipe_pane_close_on_nonexistent_pipe_is_a_no_op() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    create_session(&handler, &alpha).await;

    let response = handler
        .handle(Request::PipePane(PipePaneRequest {
            target: PaneTarget::with_window(alpha, 0, 0),
            stdin: false,
            stdout: true,
            once: false,
            command: None,
        }))
        .await;

    assert!(
        matches!(response, rmux_proto::Response::PipePane(_)),
        "closing a non-existent pipe should succeed, got {response:?}"
    );
}

#[tokio::test]
async fn pipe_pane_empty_command_closes_existing_pipe() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    create_session(&handler, &alpha).await;

    let open = handler
        .handle(Request::PipePane(PipePaneRequest {
            target: PaneTarget::with_window(alpha.clone(), 0, 0),
            stdin: false,
            stdout: true,
            once: false,
            command: Some(pipe_discard_command()),
        }))
        .await;
    assert!(matches!(open, rmux_proto::Response::PipePane(_)));

    let close = handler
        .handle(Request::PipePane(PipePaneRequest {
            target: PaneTarget::with_window(alpha.clone(), 0, 0),
            stdin: false,
            stdout: true,
            once: false,
            command: Some(String::new()),
        }))
        .await;
    assert!(
        matches!(close, rmux_proto::Response::PipePane(_)),
        "empty command should close existing pipe, got {close:?}"
    );

    // Opening a new pipe after an empty-command close should succeed, confirming the previous
    // pipe was cleaned up.
    let reopen = handler
        .handle(Request::PipePane(PipePaneRequest {
            target: PaneTarget::with_window(alpha.clone(), 0, 0),
            stdin: false,
            stdout: true,
            once: true,
            command: Some(pipe_discard_command()),
        }))
        .await;
    assert!(
        matches!(reopen, rmux_proto::Response::PipePane(_)),
        "reopening after close should succeed"
    );

    let _ = handler
        .handle(Request::PipePane(PipePaneRequest {
            target: PaneTarget::with_window(alpha, 0, 0),
            stdin: false,
            stdout: true,
            once: false,
            command: None,
        }))
        .await;
}

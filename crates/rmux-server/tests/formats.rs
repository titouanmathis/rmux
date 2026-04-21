use std::error::Error;
mod common;

use common::{send_request, session_name, start_server, TestHarness};
use rmux_proto::{
    DisplayMessageRequest, ListWindowsRequest, NewSessionRequest, NewWindowRequest, Request,
    Response, Target, TerminalSize,
};

#[tokio::test]
async fn list_windows_uses_shared_formatter_through_real_socket() -> Result<(), Box<dyn Error>> {
    let harness = TestHarness::new("formats-list-windows");
    let handle = start_server(&harness).await?;
    let alpha = session_name("alpha");

    let created = send_request(
        harness.socket_path(),
        &Request::NewSession(NewSessionRequest {
            session_name: alpha.clone(),
            detached: true,
            size: Some(TerminalSize {
                cols: 120,
                rows: 40,
            }),
            environment: Some(vec!["TERM_PROGRAM=tmux".to_owned()]),
        }),
    )
    .await?;
    assert!(matches!(created, Response::NewSession(_)));

    let new_window = send_request(
        harness.socket_path(),
        &Request::NewWindow(NewWindowRequest {
            target: alpha.clone(),
            name: Some("logs".to_owned()),
            detached: false,
            start_directory: None,
            environment: None,
            command: None,
            target_window_index: None,
            insert_at_target: false,
        }),
    )
    .await?;
    assert!(matches!(new_window, Response::NewWindow(_)));

    let listed = send_request(
        harness.socket_path(),
        &Request::ListWindows(ListWindowsRequest {
            target: alpha,
            format: Some(
                "#{session_name}:#{session_windows}:#{session_attached}:#{session_width}x#{session_height}:#{window_index}:#{window_name}:#{window_raw_flags}:#{window_active}:#{window_last_flag}:#{window_id}:#{missing}:#I#W#S:#{=21:pane_title}:#{?window_active,yes,no}"
                    .to_owned(),
            ),
            }),
    )
    .await?;

    let output = listed
        .command_output()
        .expect("list-windows returns command output");
    assert_eq!(
        std::str::from_utf8(output.stdout()).expect("list-windows output is utf-8"),
        "alpha:2:0:x:0:bash:-:0:1:@0::0bashalpha:RMUXHOST:no\n\
alpha:2:0:x:1:logs:*:1:0:@1::1logsalpha:RMUXHOST:yes\n"
    );

    handle.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn nested_conditionals_expand_inner_templates_through_real_socket(
) -> Result<(), Box<dyn Error>> {
    let harness = TestHarness::new("formats-nested-conditionals");
    let handle = start_server(&harness).await?;
    let alpha = session_name("alpha");

    let created = send_request(
        harness.socket_path(),
        &Request::NewSession(NewSessionRequest {
            session_name: alpha.clone(),
            detached: true,
            size: Some(TerminalSize {
                cols: 120,
                rows: 40,
            }),
            environment: None,
        }),
    )
    .await?;
    assert!(matches!(created, Response::NewSession(_)));

    let new_window = send_request(
        harness.socket_path(),
        &Request::NewWindow(NewWindowRequest {
            target: alpha.clone(),
            name: Some("logs".to_owned()),
            detached: false,
            start_directory: None,
            environment: None,
            command: None,
            target_window_index: None,
            insert_at_target: false,
        }),
    )
    .await?;
    assert!(matches!(new_window, Response::NewWindow(_)));

    let listed = send_request(
        harness.socket_path(),
        &Request::ListWindows(ListWindowsRequest {
            target: alpha,
            format: Some(
                "#{?window_active,#{window_name},#{?window_last_flag,last,#{session_name}}}"
                    .to_owned(),
            ),
        }),
    )
    .await?;

    let output = listed
        .command_output()
        .expect("list-windows returns command output");
    assert_eq!(
        std::str::from_utf8(output.stdout()).expect("list-windows output is utf-8"),
        "last\nlogs\n"
    );

    handle.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn display_message_session_target_includes_active_pane_runtime_context(
) -> Result<(), Box<dyn Error>> {
    let harness = TestHarness::new("formats-display-session-pane-context");
    let handle = start_server(&harness).await?;
    let alpha = session_name("alpha");

    let created = send_request(
        harness.socket_path(),
        &Request::NewSession(NewSessionRequest {
            session_name: alpha.clone(),
            detached: true,
            size: Some(TerminalSize { cols: 80, rows: 24 }),
            environment: None,
        }),
    )
    .await?;
    assert!(matches!(created, Response::NewSession(_)));

    let displayed = send_request(
        harness.socket_path(),
        &Request::DisplayMessage(DisplayMessageRequest {
            target: Some(Target::Session(alpha)),
            print: true,
            message: Some(
                "#{session_name}|#{window_index}|#{pane_index}|#{pane_current_path}|#{pane_pid}|#{pane_tty}|#{socket_path}"
                    .to_owned(),
            ),
            }),
    )
    .await?;

    let output = displayed
        .command_output()
        .expect("display-message -p returns command output");
    let stdout = std::str::from_utf8(output.stdout()).expect("display-message output is utf-8");
    let fields: Vec<&str> = stdout.trim_end().split('|').collect();
    assert_eq!(fields.len(), 7);
    assert_eq!(fields[0], "alpha");
    assert_eq!(fields[1], "0");
    assert_eq!(fields[2], "0");
    assert!(!fields[3].is_empty(), "pane_current_path must be populated");
    assert!(fields[4].parse::<u32>().is_ok(), "pane_pid must be numeric");
    assert!(fields[5].starts_with("/dev/pts/"), "pane_tty must be a pty");
    assert_eq!(fields[6], harness.socket_path().to_string_lossy());

    handle.shutdown().await?;
    Ok(())
}

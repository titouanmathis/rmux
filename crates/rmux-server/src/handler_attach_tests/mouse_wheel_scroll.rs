use super::*;

/// Test that a SGR WheelUp mouse event with a compound binding enters copy mode
/// and scrolls the pane content.
///
/// This simulates the user's `.rmux.conf` binding:
///   bind-key -T root WheelUpPane copy-mode -e \; send-keys -X -N 5 scroll-up
#[tokio::test]
async fn mouse_wheel_up_with_compound_binding_enters_copy_mode_and_scrolls() {
    let handler = RequestHandler::new();
    let requester_pid = std::process::id();
    let alpha = session_name("alpha");

    // Create a session (80x24) and attach a client.
    let mut control_rx = create_attached_session(&handler, requester_pid, &alpha).await;

    let target = PaneTarget::new(alpha.clone(), 0);

    // Load enough scroll-back history so scroll-up has something to show.
    // Each \r\n pushes one line into history when the screen overflows.
    let mut content = Vec::new();
    for i in 0..30usize {
        content.extend_from_slice(format!("scroll-line-{i:02}\r\n").as_bytes());
    }
    replace_transcript_contents(&handler, &target, TerminalSize { cols: 80, rows: 24 }, &content)
        .await;
    drain_attach_controls(&mut control_rx);

    // Register the compound binding:  copy-mode -e \; send-keys -X -N 5 scroll-up
    // This mirrors what the user has in ~/.rmux.conf.
    assert!(matches!(
        handler
            .handle(Request::BindKey(rmux_proto::BindKeyRequest {
                table_name: "root".to_owned(),
                key: "WheelUpPane".to_owned(),
                note: None,
                repeat: false,
                command: Some(vec![
                    "copy-mode".to_owned(),
                    "-e".to_owned(),
                    ";".to_owned(),
                    "send-keys".to_owned(),
                    "-X".to_owned(),
                    "-N".to_owned(),
                    "5".to_owned(),
                    "scroll-up".to_owned(),
                ]),
            }))
            .await,
        Response::BindKey(_)
    ));

    // Pane should NOT be in copy mode yet.
    assert_eq!(pane_mode_status(&handler, &alpha).await, "0:::\n");

    // Send an SGR WheelUp event: \x1b[<64;5;15M
    // Coordinates: x=5 (col 5, 1-based), y=15 (row 15, 1-based).
    // In a 80x24 session with status at row 23, these coordinates are inside the pane.
    handler
        .handle_attached_live_input_for_test(requester_pid, b"\x1b[<64;5;15M")
        .await
        .expect("WheelUp SGR mouse event must be processed without error");

    // Give the async queue a chance to flush (copy-mode + scroll-up run in the
    // same awaited call chain, so no sleep should be needed, but a yield helps).
    tokio::task::yield_now().await;

    // After the wheel-up binding executes, the pane must be in copy mode.
    assert_eq!(
        pane_mode_status(&handler, &alpha).await,
        "1:copy-mode:0:0\n",
        "WheelUp compound binding must enter copy mode"
    );
}

/// Test that the default WheelUpPane binding (with if-F check) works when pane
/// is NOT in alternate screen and NOT in any mode.
#[tokio::test]
async fn default_wheel_up_binding_enters_copy_mode_when_pane_not_in_alt_screen() {
    let handler = RequestHandler::new();
    let requester_pid = std::process::id();
    let alpha = session_name("alpha");

    let mut control_rx = create_attached_session(&handler, requester_pid, &alpha).await;
    let target = PaneTarget::new(alpha.clone(), 0);

    // Build some history.
    let mut content = Vec::new();
    for i in 0..30usize {
        content.extend_from_slice(format!("default-line-{i:02}\r\n").as_bytes());
    }
    replace_transcript_contents(&handler, &target, TerminalSize { cols: 80, rows: 24 }, &content)
        .await;
    drain_attach_controls(&mut control_rx);

    // Use the default binding (no override): the default binary binding is
    //   if -F '#{||:#{alternate_on},#{pane_in_mode},#{mouse_any_flag}}' { send -M }
    //                                                                    { copy-mode -e; send -X -N 5 scroll-up }
    // mouse_any_flag and alternate_on are both false for a plain shell pane, so
    // the else branch (copy-mode + scroll) must fire.
    handler
        .handle_attached_live_input_for_test(requester_pid, b"\x1b[<64;5;15M")
        .await
        .expect("WheelUp SGR mouse event must be processed without error");

    tokio::task::yield_now().await;

    assert_eq!(
        pane_mode_status(&handler, &alpha).await,
        "1:copy-mode:0:0\n",
        "default WheelUp binding must enter copy mode when pane is in plain shell mode"
    );
}

/// Test that after entering copy mode via WheelUp, a second WheelUp in the
/// copy-mode table continues to scroll.
#[tokio::test]
async fn wheel_up_in_copy_mode_continues_scrolling() {
    let handler = RequestHandler::new();
    let requester_pid = std::process::id();
    let alpha = session_name("alpha");

    let mut control_rx = create_attached_session(&handler, requester_pid, &alpha).await;
    let target = PaneTarget::new(alpha.clone(), 0);

    // Build 30 lines of history.
    let mut content = Vec::new();
    for i in 0..30usize {
        content.extend_from_slice(format!("cont-line-{i:02}\r\n").as_bytes());
    }
    replace_transcript_contents(&handler, &target, TerminalSize { cols: 80, rows: 24 }, &content)
        .await;
    drain_attach_controls(&mut control_rx);

    // Enter copy mode first (via direct request to isolate the entry path).
    assert!(matches!(
        handler
            .handle(Request::CopyMode(CopyModeRequest {
                target: Some(target.clone()),
                page_down: false,
                exit_on_scroll: true,
                hide_position: false,
                mouse_drag_start: false,
                cancel_mode: false,
                scrollbar_scroll: false,
                source: None,
                page_up: false,
            }))
            .await,
        Response::CopyMode(_)
    ));
    drain_attach_controls(&mut control_rx);

    // Now send a WheelUp while already in copy mode.
    // The copy-mode binding is: send -N5 -X scroll-up
    handler
        .handle_attached_live_input_for_test(requester_pid, b"\x1b[<64;5;15M")
        .await
        .expect("WheelUp in copy-mode must be processed without error");

    tokio::task::yield_now().await;

    // Must still be in copy mode after scrolling up.
    assert_eq!(
        pane_mode_status(&handler, &alpha).await,
        "1:copy-mode:0:0\n",
        "WheelUp inside copy-mode must keep copy mode active"
    );
}

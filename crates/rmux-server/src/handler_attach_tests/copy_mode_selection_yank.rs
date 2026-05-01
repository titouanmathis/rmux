use super::*;

const ORACLE_YANK_BYTES: &[u8] = b"alpha ";

async fn set_vi_mode_keys(handler: &RequestHandler, session: &SessionName) {
    assert!(matches!(
        handler
            .handle(Request::SetOption(SetOptionRequest {
                scope: ScopeSelector::Window(WindowTarget::with_window(session.clone(), 0)),
                option: OptionName::ModeKeys,
                value: "vi".to_owned(),
                mode: SetOptionMode::Replace,
            }))
            .await,
        Response::SetOption(_)
    ));
}

async fn enter_copy_mode_with_selection_seed(
    handler: &RequestHandler,
    target: &PaneTarget,
) -> String {
    replace_transcript_contents(
        handler,
        target,
        TerminalSize { cols: 80, rows: 24 },
        b"alpha beta gamma\r\nsecond beta line\r\nthird alpha marker\r\nfourth delta marker\r\nfifth beta tail\r\n\x1b[1;1H",
    )
    .await;
    assert!(matches!(
        handler
            .handle(Request::CopyMode(CopyModeRequest {
                target: Some(target.clone()),
                page_down: false,
                exit_on_scroll: false,
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
    copy_selection_status(handler, target.clone()).await
}

async fn copy_selection_status(handler: &RequestHandler, target: PaneTarget) -> String {
    display_target_format(
        handler,
        target,
        "#{pane_in_mode}:#{copy_cursor_x},#{copy_cursor_y}:#{selection_present}:#{selection_active}:#{selection_mode}:#{selection_start_x},#{selection_start_y}:#{selection_end_x},#{selection_end_y}",
    )
    .await
}

async fn send_copy_selection_key(
    handler: &RequestHandler,
    requester_pid: u32,
    pending_input: &mut Vec<u8>,
    bytes: &[u8],
) {
    let forwarded_to_pane = handler
        .handle_attached_live_input_inner(requester_pid, pending_input, bytes)
        .await
        .expect("copy-mode selection input");
    assert!(
        !forwarded_to_pane,
        "copy-mode selection/yank keys must be consumed instead of forwarded to pane IO"
    );
    assert!(
        pending_input.is_empty(),
        "copy-mode selection/yank input should fully decode and leave no pending bytes"
    );
}

async fn show_top_buffer_bytes(handler: &RequestHandler) -> Vec<u8> {
    let response = handler
        .handle(Request::ShowBuffer(rmux_proto::ShowBufferRequest {
            name: None,
        }))
        .await;
    let Response::ShowBuffer(response) = response else {
        panic!("expected show-buffer response, got {response:?}");
    };
    response.command_output().stdout().to_vec()
}

async fn enter_vi_selection_yank_fixture(
    handler: &RequestHandler,
    requester_pid: u32,
    session: &SessionName,
    target: &PaneTarget,
) -> (Vec<u8>, String) {
    set_vi_mode_keys(handler, session).await;
    assert_eq!(
        enter_copy_mode_with_selection_seed(handler, target).await,
        "1:0,0:0:0::,:,\n"
    );
    let before_capture = capture_pane_print(handler, target.clone()).await;
    let mut pending_input = Vec::new();

    send_copy_selection_key(handler, requester_pid, &mut pending_input, b" ").await;
    assert_eq!(
        copy_selection_status(handler, target.clone()).await,
        "1:0,0:1:1:char:0,0:0,0\n"
    );

    for expected_x in 1..=5 {
        send_copy_selection_key(handler, requester_pid, &mut pending_input, b"\x1b[C").await;
        assert_eq!(
            copy_selection_status(handler, target.clone()).await,
            format!("1:{expected_x},0:1:1:char:0,0:{expected_x},0\n")
        );
    }

    (pending_input, before_capture)
}

#[tokio::test]
async fn vi_copy_mode_selection_begin_marks_anchor_without_pane_leak() {
    let handler = RequestHandler::new();
    let requester_pid = std::process::id();
    let alpha = session_name("alpha");
    let _control_rx = create_quiet_attached_session(&handler, requester_pid, &alpha).await;
    let target = PaneTarget::new(alpha.clone(), 0);

    let (_pending_input, before_capture) =
        enter_vi_selection_yank_fixture(&handler, requester_pid, &alpha, &target).await;

    assert_eq!(
        capture_pane_print(&handler, target).await,
        before_capture,
        "selection begin and motion keys must not mutate the pane screen"
    );
}

#[tokio::test]
async fn vi_copy_mode_selection_yank_writes_internal_buffer_matching_tmux() {
    let handler = RequestHandler::new();
    let requester_pid = std::process::id();
    let alpha = session_name("alpha");
    let _control_rx = create_quiet_attached_session(&handler, requester_pid, &alpha).await;
    let target = PaneTarget::new(alpha.clone(), 0);

    let (mut pending_input, before_capture) =
        enter_vi_selection_yank_fixture(&handler, requester_pid, &alpha, &target).await;

    send_copy_selection_key(&handler, requester_pid, &mut pending_input, b"\r").await;
    assert_eq!(
        copy_selection_status(&handler, target.clone()).await,
        "0:,::::,:,\n",
        "vi Enter must copy the selection and exit copy-mode like tmux"
    );
    assert_eq!(
        show_top_buffer_bytes(&handler).await,
        ORACLE_YANK_BYTES,
        "RMUX internal buffer must match tmux save-buffer bytes exactly"
    );
    assert_eq!(
        capture_pane_print(&handler, target.clone()).await,
        before_capture,
        "selection/yank keys must not reach or mutate pane IO"
    );

    let forwarded_to_pane = handler
        .handle_attached_live_input_inner(
            requester_pid,
            &mut pending_input,
            b"RMUX_AFTER_COPY_SELECTION_YANK",
        )
        .await
        .expect("normal input resumes after copy-mode yank");
    assert!(
        forwarded_to_pane,
        "normal pane input should resume after copy-mode yank exits"
    );
}

#[tokio::test]
async fn copy_mode_selection_yank_does_not_depend_on_search() {
    let handler = RequestHandler::new();
    let requester_pid = std::process::id();
    let alpha = session_name("alpha");
    let _control_rx = create_quiet_attached_session(&handler, requester_pid, &alpha).await;
    let target = PaneTarget::new(alpha.clone(), 0);

    let (mut pending_input, _before_capture) =
        enter_vi_selection_yank_fixture(&handler, requester_pid, &alpha, &target).await;
    assert_eq!(
        copy_selection_status(&handler, target.clone()).await,
        "1:5,0:1:1:char:0,0:5,0\n",
        "the W3C slice positions by motion only before yanking"
    );

    send_copy_selection_key(&handler, requester_pid, &mut pending_input, b"\r").await;
    assert_eq!(show_top_buffer_bytes(&handler).await, ORACLE_YANK_BYTES);
}

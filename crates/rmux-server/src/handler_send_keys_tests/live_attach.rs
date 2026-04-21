use super::*;

#[tokio::test]
async fn send_keys_uses_runtime_extended_key_format_for_mode_two() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");

    let created = handler
        .handle(Request::NewSession(NewSessionRequest {
            session_name: alpha.clone(),
            detached: true,
            size: Some(TerminalSize { cols: 80, rows: 24 }),
            environment: None,
        }))
        .await;
    assert!(matches!(created, Response::NewSession(_)));

    let set_format = handler
        .handle(Request::SetOption(SetOptionRequest {
            scope: ScopeSelector::Global,
            option: OptionName::ExtendedKeysFormat,
            value: "csi-u".to_owned(),
            mode: SetOptionMode::Replace,
        }))
        .await;
    assert!(matches!(set_format, Response::SetOption(_)));

    {
        let mut state = handler.state.lock().await;
        state
            .append_bytes_to_pane_transcript_for_test(&alpha, 0, 0, b"\x1b[>4;2m")
            .expect("mode 2 transcript update");
    }

    let output_path = unique_output_path("extended-key");
    start_cat_capture(&handler, &alpha, &output_path).await;

    let response = handler
        .handle(Request::SendKeysExt(SendKeysExtRequest {
            target: Some(PaneTarget::new(alpha.clone(), 0)),
            keys: vec!["M-C-a".to_owned()],
            expand_formats: false,
            hex: false,
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

    finish_cat_capture(&handler, &alpha).await;

    let expected = encode_key(
        mode::MODE_KEYS_EXTENDED_2,
        ExtendedKeyFormat::CsiU,
        key_string_lookup_string("M-C-a").expect("key parses"),
    )
    .expect("extended key encodes");
    wait_for_file_bytes(&output_path, &expected)
        .await
        .expect("extended key file contents");
    let _ = fs::remove_file(&output_path);
}

#[tokio::test]
async fn send_keys_m_forwards_the_current_mouse_event_to_the_pane() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    let requester_pid = std::process::id();

    let created = handler
        .handle(Request::NewSession(NewSessionRequest {
            session_name: alpha.clone(),
            detached: true,
            size: Some(TerminalSize { cols: 80, rows: 24 }),
            environment: None,
        }))
        .await;
    assert!(matches!(created, Response::NewSession(_)));

    {
        let mut state = handler.state.lock().await;
        state
            .append_bytes_to_pane_transcript_for_test(&alpha, 0, 0, b"\x1b[?1000h")
            .expect("mouse mode transcript update");
    }

    let (control_tx, _control_rx) = mpsc::unbounded_channel();
    let _attach_id = handler
        .register_attach(requester_pid, alpha.clone(), control_tx)
        .await;

    let (window_id, pane_id, pane_target) = {
        let state = handler.state.lock().await;
        let session = state.sessions.session(&alpha).expect("session exists");
        let window = session.window_at(0).expect("window exists");
        let pane = window.pane(0).expect("pane exists");
        (window.id(), pane.id(), PaneTarget::new(alpha.clone(), 0))
    };

    let raw = MouseForwardEvent {
        b: 0,
        lb: 0,
        x: 1,
        y: 1,
        lx: 1,
        ly: 1,
        sgr_b: 0,
        sgr_type: ' ',
        ignore: false,
    };
    {
        let mut active_attach = handler.active_attach.lock().await;
        let active = active_attach
            .by_pid
            .get_mut(&requester_pid)
            .expect("attached client exists");
        active.mouse.current_event = Some(AttachedMouseEvent {
            raw,
            session_id: 0,
            window_id: Some(window_id),
            pane_id: Some(pane_id),
            pane_target: Some(pane_target.clone()),
            location: MouseLocation::Pane,
            status_at: None,
            status_lines: 0,
            ignore: false,
        });
    }

    let output_path = unique_output_path("mouse-forward");
    start_cat_capture(&handler, &alpha, &output_path).await;

    let response = handler
        .handle(Request::SendKeysExt(SendKeysExtRequest {
            target: Some(pane_target),
            keys: Vec::new(),
            expand_formats: false,
            hex: false,
            literal: false,
            dispatch_key_table: false,
            copy_mode_command: false,
            forward_mouse_event: true,
            reset_terminal: false,
            repeat_count: None,
        }))
        .await;
    assert_eq!(
        response,
        Response::SendKeys(SendKeysResponse { key_count: 0 })
    );

    finish_cat_capture(&handler, &alpha).await;

    let expected =
        encode_mouse_event(mode::MODE_MOUSE_STANDARD, &raw, raw.x, raw.y).expect("mouse encodes");
    wait_for_file_bytes(&output_path, &expected)
        .await
        .expect("mouse file contents");
    let _ = fs::remove_file(&output_path);
}

#[tokio::test]
async fn live_attach_extended_keys_are_reencoded_for_the_target_pane() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    let requester_pid = std::process::id();

    let created = handler
        .handle(Request::NewSession(NewSessionRequest {
            session_name: alpha.clone(),
            detached: true,
            size: Some(TerminalSize { cols: 80, rows: 24 }),
            environment: None,
        }))
        .await;
    assert!(matches!(created, Response::NewSession(_)));

    let (control_tx, _control_rx) = mpsc::unbounded_channel();
    let _attach_id = handler
        .register_attach(requester_pid, alpha.clone(), control_tx)
        .await;

    let output_path = unique_output_path("live-attach-extended-key");
    start_cat_capture(&handler, &alpha, &output_path).await;

    handler
        .handle_attached_live_input_for_test(requester_pid, b"\x1b[9;2u")
        .await
        .expect("live attach input");

    finish_cat_capture(&handler, &alpha).await;

    wait_for_file_bytes(&output_path, b"\x1b[Z")
        .await
        .expect("extended key file contents");
    let _ = fs::remove_file(&output_path);
}

#[tokio::test]
async fn live_attach_bracketed_paste_sequences_pass_through_unchanged_when_chunked() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    let requester_pid = std::process::id();

    let created = handler
        .handle(Request::NewSession(NewSessionRequest {
            session_name: alpha.clone(),
            detached: true,
            size: Some(TerminalSize { cols: 80, rows: 24 }),
            environment: None,
        }))
        .await;
    assert!(matches!(created, Response::NewSession(_)));

    let (control_tx, _control_rx) = mpsc::unbounded_channel();
    let _attach_id = handler
        .register_attach(requester_pid, alpha.clone(), control_tx)
        .await;

    let output_path = unique_output_path("live-attach-bracketed-paste");
    start_cat_capture(&handler, &alpha, &output_path).await;

    let mut pending_input = Vec::new();
    handler
        .handle_attached_live_input(requester_pid, &mut pending_input, b"\x1b[20")
        .await
        .expect("first bracketed paste chunk");
    assert_eq!(pending_input, b"\x1b[20");

    handler
        .handle_attached_live_input(requester_pid, &mut pending_input, b"0~paste\x1b[201~")
        .await
        .expect("second bracketed paste chunk");
    assert!(pending_input.is_empty());

    finish_cat_capture(&handler, &alpha).await;

    wait_for_file_bytes(&output_path, b"\x1b[200~paste\x1b[201~")
        .await
        .expect("bracketed paste file contents");
    let _ = fs::remove_file(&output_path);
}

#[tokio::test]
async fn live_attach_focus_sequences_pass_through_unchanged() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    let requester_pid = std::process::id();

    let created = handler
        .handle(Request::NewSession(NewSessionRequest {
            session_name: alpha.clone(),
            detached: true,
            size: Some(TerminalSize { cols: 80, rows: 24 }),
            environment: None,
        }))
        .await;
    assert!(matches!(created, Response::NewSession(_)));

    let (control_tx, _control_rx) = mpsc::unbounded_channel();
    let _attach_id = handler
        .register_attach(requester_pid, alpha.clone(), control_tx)
        .await;

    let output_path = unique_output_path("live-attach-focus");
    start_cat_capture(&handler, &alpha, &output_path).await;

    handler
        .handle_attached_live_input_for_test(requester_pid, b"\x1b[I\x1b[O")
        .await
        .expect("live attach focus input");

    finish_cat_capture(&handler, &alpha).await;

    wait_for_file_bytes(&output_path, b"\x1b[I\x1b[O")
        .await
        .expect("focus sequence file contents");
    let _ = fs::remove_file(&output_path);
}

#[tokio::test]
async fn live_attach_mouse_sequences_dispatch_default_mouse_bindings() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    let requester_pid = std::process::id();

    let created = handler
        .handle(Request::NewSession(NewSessionRequest {
            session_name: alpha.clone(),
            detached: true,
            size: Some(TerminalSize { cols: 80, rows: 24 }),
            environment: None,
        }))
        .await;
    assert!(matches!(created, Response::NewSession(_)));

    {
        let mut state = handler.state.lock().await;
        state
            .append_bytes_to_pane_transcript_for_test(&alpha, 0, 0, b"\x1b[?1002h")
            .expect("mouse motion mode transcript update");
    }

    let (control_tx, _control_rx) = mpsc::unbounded_channel();
    let _attach_id = handler
        .register_attach(requester_pid, alpha.clone(), control_tx)
        .await;

    let rebound = handler
        .handle(Request::BindKey(BindKeyRequest {
            table_name: "root".to_owned(),
            key: "MouseDrag1Pane".to_owned(),
            note: Some("live-attach-mouse".to_owned()),
            repeat: false,
            command: Some(vec!["send-keys".to_owned(), "-M".to_owned()]),
        }))
        .await;
    assert!(matches!(rebound, Response::BindKey(_)));

    let output_path = unique_output_path("live-attach-mouse");
    start_cat_capture(&handler, &alpha, &output_path).await;

    handler
        .handle_attached_live_input_for_test(requester_pid, b"\x1b[<32;2;2M")
        .await
        .expect("live attach mouse input");

    finish_cat_capture(&handler, &alpha).await;

    let expected = encode_mouse_event(
        mode::MODE_MOUSE_BUTTON,
        &MouseForwardEvent {
            b: 32,
            lb: 0,
            x: 1,
            y: 1,
            lx: 0,
            ly: 0,
            sgr_b: 32,
            sgr_type: 'M',
            ignore: false,
        },
        1,
        1,
    )
    .expect("mouse encodes");
    wait_for_file_bytes(&output_path, &expected)
        .await
        .expect("mouse file contents");
    let _ = fs::remove_file(&output_path);

    let active_attach = handler.active_attach.lock().await;
    let event = active_attach
        .by_pid
        .get(&requester_pid)
        .and_then(|active| active.mouse.current_event.as_ref())
        .expect("current mouse event");
    assert_eq!(event.location, MouseLocation::Pane);
}

#[tokio::test]
async fn live_attach_manual_prompt_drag_sequence_does_not_error() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    let requester_pid = std::process::id();

    let created = handler
        .handle(Request::NewSession(NewSessionRequest {
            session_name: alpha.clone(),
            detached: true,
            size: Some(TerminalSize { cols: 80, rows: 24 }),
            environment: None,
        }))
        .await;
    assert!(matches!(created, Response::NewSession(_)));

    let (control_tx, _control_rx) = mpsc::unbounded_channel();
    let _attach_id = handler
        .register_attach(requester_pid, alpha, control_tx)
        .await;

    let result = handler
        .handle_attached_live_input_for_test(
            requester_pid,
            b"\x1b[<0;7;1M\x1b[<32;9;1M\x1b[<32;10;1M",
        )
        .await;
    assert!(result.is_ok(), "{result:?}");
}

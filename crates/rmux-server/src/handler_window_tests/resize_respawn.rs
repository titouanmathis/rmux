use super::*;

#[tokio::test]
async fn resize_window_applies_explicit_dimensions() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    create_session(&handler, "alpha").await;

    let response = handler
        .handle(Request::ResizeWindow(ResizeWindowRequest {
            target: WindowTarget::with_window(alpha.clone(), 0),
            width: Some(60),
            height: Some(20),
            adjustment: None,
        }))
        .await;

    assert!(
        matches!(&response, Response::ResizeWindow(r) if r.target == WindowTarget::with_window(alpha.clone(), 0)),
        "expected resize success, got {response:?}"
    );

    let state = handler.state.lock().await;
    let session = state.sessions.session(&alpha).expect("alpha should exist");
    let window = session.window_at(0).expect("window 0 should exist");
    assert_eq!(window.size().cols, 60);
    assert_eq!(window.size().rows, 20);
}

#[tokio::test]
async fn resize_window_applies_relative_adjustment() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    create_session(&handler, "alpha").await;

    // Session created with cols=120, rows=40. Shrink by 10 cols.
    let response = handler
        .handle(Request::ResizeWindow(ResizeWindowRequest {
            target: WindowTarget::with_window(alpha.clone(), 0),
            width: None,
            height: None,
            adjustment: Some(ResizeWindowAdjustment::Left(10)),
        }))
        .await;

    assert!(matches!(response, Response::ResizeWindow(_)));

    let state = handler.state.lock().await;
    let session = state.sessions.session(&alpha).expect("alpha should exist");
    let window = session.window_at(0).expect("window 0 should exist");
    assert_eq!(window.size().cols, 110);
    assert_eq!(window.size().rows, 40);
}

#[tokio::test]
async fn resize_window_applies_adjustment_after_explicit_dimensions() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    create_session(&handler, "alpha").await;

    let response = handler
        .handle(Request::ResizeWindow(ResizeWindowRequest {
            target: WindowTarget::with_window(alpha.clone(), 0),
            width: Some(60),
            height: Some(20),
            adjustment: Some(ResizeWindowAdjustment::Down(5)),
        }))
        .await;

    assert!(matches!(response, Response::ResizeWindow(_)));

    let state = handler.state.lock().await;
    let session = state.sessions.session(&alpha).expect("alpha should exist");
    let window = session.window_at(0).expect("window 0 should exist");
    assert_eq!(window.size().cols, 60);
    assert_eq!(window.size().rows, 25);
}

#[tokio::test]
async fn resize_window_clamps_relative_adjustments_to_a_minimum_size_of_one() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    create_session(&handler, "alpha").await;

    let response = handler
        .handle(Request::ResizeWindow(ResizeWindowRequest {
            target: WindowTarget::with_window(alpha.clone(), 0),
            width: Some(2),
            height: Some(3),
            adjustment: Some(ResizeWindowAdjustment::Left(10)),
        }))
        .await;

    assert!(matches!(response, Response::ResizeWindow(_)));

    let state = handler.state.lock().await;
    let session = state.sessions.session(&alpha).expect("alpha should exist");
    let window = session.window_at(0).expect("window 0 should exist");
    assert_eq!(window.size().cols, 1);
    assert_eq!(window.size().rows, 3);
}

#[tokio::test]
async fn resize_window_rejects_nonexistent_window() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    create_session(&handler, "alpha").await;

    let response = handler
        .handle(Request::ResizeWindow(ResizeWindowRequest {
            target: WindowTarget::with_window(alpha.clone(), 99),
            width: Some(40),
            height: Some(20),
            adjustment: None,
        }))
        .await;

    assert!(
        matches!(response, Response::Error(_)),
        "expected error for nonexistent window, got {response:?}"
    );
}

#[tokio::test]
async fn respawn_window_rejects_active_window_without_kill_flag() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    create_session(&handler, "alpha").await;

    // Window 0 has a running pane — respawn without -k should fail.
    let response = handler
        .handle(Request::RespawnWindow(RespawnWindowRequest {
            target: WindowTarget::with_window(alpha.clone(), 0),
            kill: false,
            start_directory: None,
            environment: None,
            command: None,
        }))
        .await;

    assert!(
        matches!(&response, Response::Error(e) if e.error.to_string().contains("still active")),
        "expected still-active error, got {response:?}"
    );
}

#[tokio::test]
async fn respawn_window_succeeds_with_kill_flag() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    create_session(&handler, "alpha").await;

    let response = handler
        .handle(Request::RespawnWindow(RespawnWindowRequest {
            target: WindowTarget::with_window(alpha.clone(), 0),
            kill: true,
            start_directory: None,
            environment: None,
            command: None,
        }))
        .await;

    assert!(
        matches!(&response, Response::RespawnWindow(r) if r.target == WindowTarget::with_window(alpha.clone(), 0)),
        "expected respawn success with -k, got {response:?}"
    );

    // After respawn, window should still exist with exactly one pane.
    let state = handler.state.lock().await;
    let session = state.sessions.session(&alpha).expect("alpha should exist");
    let window = session
        .window_at(0)
        .expect("window 0 should exist after respawn");
    assert_eq!(window.panes().len(), 1);
}

#[tokio::test]
async fn respawn_window_selects_target_window_like_tmux() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    create_session(&handler, "alpha").await;
    insert_window(&handler, &alpha, 1).await;

    assert!(matches!(
        handler
            .handle(Request::SelectWindow(SelectWindowRequest {
                target: WindowTarget::with_window(alpha.clone(), 1),
            }))
            .await,
        Response::SelectWindow(_)
    ));

    let response = handler
        .handle(Request::RespawnWindow(RespawnWindowRequest {
            target: WindowTarget::with_window(alpha.clone(), 0),
            kill: true,
            start_directory: None,
            environment: None,
            command: None,
        }))
        .await;

    assert!(
        matches!(response, Response::RespawnWindow(_)),
        "respawn-window should succeed, got {response:?}"
    );

    let state = handler.state.lock().await;
    let session = state.sessions.session(&alpha).expect("alpha should exist");
    assert_eq!(session.active_window_index(), 0);
}

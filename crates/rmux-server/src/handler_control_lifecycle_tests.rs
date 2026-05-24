use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use super::RequestHandler;
use crate::control::{ControlModeUpgrade, ControlServerEvent};
use rmux_proto::{
    ClientTerminalContext, ControlMode, KillSessionRequest, NewSessionRequest, Request, Response,
    SessionName, TerminalSize,
};
use tokio::sync::mpsc;

fn session_name(value: &str) -> SessionName {
    SessionName::new(value).expect("valid session name")
}

async fn new_session(handler: &RequestHandler, session_name: &SessionName) {
    let response = handler
        .handle(Request::NewSession(NewSessionRequest {
            session_name: session_name.clone(),
            detached: true,
            size: Some(TerminalSize { cols: 80, rows: 24 }),
            environment: None,
        }))
        .await;
    assert!(matches!(response, Response::NewSession(_)));
}

async fn register_control_session(
    handler: &RequestHandler,
    requester_pid: u32,
    session_name: SessionName,
) -> mpsc::UnboundedReceiver<ControlServerEvent> {
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let _control_id = handler
        .register_control_with_closing(
            requester_pid,
            ControlModeUpgrade {
                mode: ControlMode::Plain,
                terminal_context: crate::outer_terminal::OuterTerminalContext::default()
                    .with_client_terminal(&ClientTerminalContext {
                        terminal_features: Vec::new(),
                        utf8: true,
                    }),
            },
            event_tx,
            Arc::new(AtomicBool::new(false)),
        )
        .await;
    handler
        .set_control_session(requester_pid, Some(session_name))
        .await
        .expect("control session set succeeds");
    event_rx
}

async fn dispatch_as(handler: &RequestHandler, requester_pid: u32, request: Request) -> Response {
    let mut lifecycle_events = handler.subscribe_lifecycle_events();
    let outcome = handler.dispatch(requester_pid, request).await;

    loop {
        match lifecycle_events.try_recv() {
            Ok(event) => handler.dispatch_lifecycle_hook(event).await,
            Err(
                tokio::sync::broadcast::error::TryRecvError::Empty
                | tokio::sync::broadcast::error::TryRecvError::Closed,
            ) => break,
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(skipped)) => {
                panic!("lifecycle events lagged during test: {skipped}");
            }
        }
    }

    outcome.response
}

#[tokio::test]
async fn control_client_exits_when_its_target_session_is_killed() {
    let handler = RequestHandler::new();
    let alpha = session_name("alpha");
    let requester_pid = 4242;
    new_session(&handler, &alpha).await;
    let mut rx = register_control_session(&handler, requester_pid, alpha.clone()).await;
    while rx.try_recv().is_ok() {}

    let response = dispatch_as(
        &handler,
        requester_pid,
        Request::KillSession(KillSessionRequest {
            target: alpha,
            kill_all_except_target: false,
            clear_alerts: false,
        }),
    )
    .await;
    assert!(matches!(response, Response::KillSession(_)));

    let events = {
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        events
    };

    assert!(
        events
            .iter()
            .any(|event| matches!(event, ControlServerEvent::Exit(None))),
        "control client must receive %exit after target deletion, got {events:?}"
    );
}

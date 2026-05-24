use rmux_proto::{
    DaemonStatusResponse, KillServerResponse, Response, ShutdownIfIdleResponse, RMUX_WIRE_VERSION,
};
use std::sync::atomic::Ordering;

use super::{PendingShutdownReason, RequestHandler};

impl RequestHandler {
    pub(in crate::handler) async fn handle_daemon_status(&self) -> Response {
        let (session_count, client_count) = self.daemon_activity_counts().await;
        Response::DaemonStatus(DaemonStatusResponse {
            rmux_version: env!("CARGO_PKG_VERSION").to_owned(),
            wire_version: RMUX_WIRE_VERSION,
            session_count,
            client_count,
        })
    }

    pub(in crate::handler) async fn handle_shutdown_if_idle(&self) -> Response {
        let (session_count, client_count) = self.daemon_activity_counts().await;
        let shutdown = session_count == 0 && client_count == 0;
        if shutdown {
            self.retained_exited_outputs
                .lock()
                .expect("retained exited output mutex must not be poisoned")
                .clear();
            self.queue_shutdown_request(PendingShutdownReason::SeamlessUpgradeIdle);
        }

        Response::ShutdownIfIdle(ShutdownIfIdleResponse {
            shutdown,
            session_count,
            client_count,
        })
    }

    pub(in crate::handler) async fn handle_kill_server(&self) -> Response {
        self.retained_exited_outputs
            .lock()
            .expect("retained exited output mutex must not be poisoned")
            .clear();
        self.queue_shutdown_request(PendingShutdownReason::KillServer);
        Response::KillServer(KillServerResponse)
    }

    async fn daemon_activity_counts(&self) -> (usize, usize) {
        let session_count = {
            let state = self.state.lock().await;
            state.sessions.len()
        };
        let attach_count = self.active_attach.lock().await.by_pid.len();
        let control_count = self.active_control.lock().await.by_pid.len();
        let detached_request_count = self.active_detached_requests.load(Ordering::SeqCst);
        (
            session_count,
            attach_count + control_count + detached_request_count,
        )
    }
}

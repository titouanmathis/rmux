#![cfg(windows)]

use std::error::Error;
use std::ffi::OsString;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use rmux_client::{
    connect, ensure_server_running, socket_path_for_label, Connection, INTERNAL_DAEMON_FLAG,
};
use rmux_proto::{KillServerRequest, ListSessionsRequest, Request, Response};

const BINARY_OVERRIDE_ENV: &str = "RMUX_INTERNAL_BINARY_PATH";
const BINARY_OVERRIDE_TEST_OPT_IN_ENV: &str = "RMUX_ALLOW_INTERNAL_BINARY_OVERRIDE";

#[test]
fn hidden_daemon_mode_serves_windows_ipc_requests() -> Result<(), Box<dyn Error>> {
    let socket_path =
        socket_path_for_label(format!("hidden-daemon-windows-{}", std::process::id()))?;
    let mut child = Command::new(env!("CARGO_BIN_EXE_rmux"))
        .arg(INTERNAL_DAEMON_FLAG)
        .arg(&socket_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    let mut connection = match wait_for_connection(&socket_path, &mut child) {
        Ok(connection) => connection,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };

    let response = connection.roundtrip(&Request::ListSessions(ListSessionsRequest {
        format: None,
        filter: None,
        sort_order: None,
        reversed: false,
    }))?;
    let Response::ListSessions(response) = response else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("list-sessions did not return a list-sessions response".into());
    };
    assert!(response.output.stdout().is_empty());

    let response = connection.roundtrip(&Request::KillServer(KillServerRequest))?;
    assert!(matches!(response, Response::KillServer(_)));
    wait_for_child_exit(&mut child)?;
    Ok(())
}

#[test]
fn ensure_server_running_auto_starts_windows_hidden_daemon() -> Result<(), Box<dyn Error>> {
    let _guard = env_lock().lock().expect("lock env");
    let previous_binary = std::env::var_os(BINARY_OVERRIDE_ENV);
    let previous_opt_in = std::env::var_os(BINARY_OVERRIDE_TEST_OPT_IN_ENV);
    let _binary_restore = EnvVarGuard::new(BINARY_OVERRIDE_ENV, previous_binary);
    let _opt_in_restore = EnvVarGuard::new(BINARY_OVERRIDE_TEST_OPT_IN_ENV, previous_opt_in);
    std::env::set_var(BINARY_OVERRIDE_ENV, env!("CARGO_BIN_EXE_rmux"));
    std::env::set_var(BINARY_OVERRIDE_TEST_OPT_IN_ENV, "1");
    let socket_path = socket_path_for_label(format!("auto-start-windows-{}", std::process::id()))?;

    let mut connection = ensure_server_running(&socket_path)?;
    let response = connection.roundtrip(&Request::ListSessions(ListSessionsRequest {
        format: None,
        filter: None,
        sort_order: None,
        reversed: false,
    }))?;
    assert!(matches!(response, Response::ListSessions(_)));

    let response = connection.roundtrip(&Request::KillServer(KillServerRequest))?;
    assert!(matches!(response, Response::KillServer(_)));
    Ok(())
}

fn wait_for_connection(
    socket_path: &std::path::Path,
    child: &mut Child,
) -> Result<Connection, Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);

    loop {
        if let Some(status) = child.try_wait()? {
            return Err(format!("hidden daemon exited before accepting IPC: {status}").into());
        }

        match connect(socket_path) {
            Ok(connection) => return Ok(connection),
            Err(error) if Instant::now() < deadline => {
                let _ = error;
                thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(Box::new(error)),
        }
    }
}

fn wait_for_child_exit(child: &mut Child) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);

    loop {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            child.kill()?;
            let _ = child.wait();
            return Err("hidden daemon did not exit after kill-server".into());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvVarGuard {
    name: &'static str,
    previous_value: Option<OsString>,
}

impl EnvVarGuard {
    fn new(name: &'static str, previous_value: Option<OsString>) -> Self {
        Self {
            name,
            previous_value,
        }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.previous_value.as_ref() {
            Some(value) => std::env::set_var(self.name, value),
            None => std::env::remove_var(self.name),
        }
    }
}

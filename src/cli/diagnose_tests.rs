use super::*;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvVarGuard {
    name: &'static str,
    value: Option<OsString>,
}

impl EnvVarGuard {
    fn capture(name: &'static str) -> Self {
        Self {
            name,
            value: std::env::var_os(name),
        }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.value.as_ref() {
            Some(value) => std::env::set_var(self.name, value),
            None => std::env::remove_var(self.name),
        }
    }
}

#[test]
fn parser_detects_diagnose_after_socket_flags() {
    let invocation = parse_invocation(&[
        OsString::from("-Ldiag"),
        OsString::from("-Tclipboard,RGB"),
        OsString::from("diagnose"),
        OsString::from("--json"),
    ])
    .expect("parse diagnose")
    .expect("diagnose invocation");

    assert_eq!(invocation.format, DiagnoseFormat::Json);
    assert_eq!(invocation.socket_name, Some(OsString::from("diag")));
    assert_eq!(
        invocation.terminal_features,
        vec!["clipboard".to_owned(), "RGB".to_owned()]
    );
}

#[test]
fn parser_ignores_non_diagnose_commands() {
    assert_eq!(
        parse_invocation(&[OsString::from("new-session")]).expect("parse"),
        None
    );
}

#[test]
fn json_renderer_escapes_strings() {
    assert_eq!(json_string("a\"b\\c\n"), "\"a\\\"b\\\\c\\n\"");
}

#[test]
fn path_redaction_replaces_home_prefix() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let _home = EnvVarGuard::capture("HOME");
    let home = std::env::temp_dir().join("rmux-diagnose-home");
    std::env::set_var("HOME", &home);

    assert_eq!(redact_path(&home.join("rmux.conf")), "~/rmux.conf");
}

#[cfg(windows)]
#[test]
fn detected_shell_reports_the_windows_default_pane_shell() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let _path = EnvVarGuard::capture("PATH");
    let _comspec = EnvVarGuard::capture("COMSPEC");
    let root = std::env::temp_dir().join(format!("rmux-diagnose-shell-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("temp shell dir");
    let pwsh = root.join("pwsh.exe");
    std::fs::write(&pwsh, b"").expect("fake pwsh");
    std::env::set_var("PATH", &root);
    std::env::set_var("COMSPEC", r"C:\Windows\System32\cmd.exe");

    assert_eq!(detected_shell(), pwsh.to_string_lossy());

    std::fs::remove_dir_all(root).expect("remove temp shell dir");
}

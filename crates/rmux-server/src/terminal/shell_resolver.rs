use std::collections::HashMap;
#[cfg(windows)]
use std::env;
#[cfg(unix)]
use std::fs;
#[cfg(windows)]
use std::path::Path;
use std::path::PathBuf;

use rmux_core::OptionStore;
use rmux_proto::{OptionName, SessionName};
#[cfg(unix)]
use rustix::process::getuid;

#[cfg(unix)]
pub(super) fn resolve_shell_path(
    options: &OptionStore,
    session_name: Option<&SessionName>,
    environment: &HashMap<String, String>,
) -> PathBuf {
    session_name
        .and_then(|session_name| options.resolve(Some(session_name), OptionName::DefaultShell))
        .or_else(|| options.resolve(None, OptionName::DefaultShell))
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(normalize_shell_path)
        .or_else(current_user_login_shell)
        .or_else(|| environment.get("SHELL").map(PathBuf::from))
        .map(normalize_shell_path)
        .unwrap_or_else(default_shell_path)
}

#[cfg(windows)]
pub(super) fn resolve_shell_path(
    options: &OptionStore,
    session_name: Option<&SessionName>,
    _environment: &HashMap<String, String>,
) -> PathBuf {
    explicit_default_shell(options, session_name)
        .map(PathBuf::from)
        .map(normalize_shell_path)
        .unwrap_or_else(default_shell_path)
}

#[cfg(windows)]
fn explicit_default_shell<'a>(
    options: &'a OptionStore,
    session_name: Option<&SessionName>,
) -> Option<&'a str> {
    session_name
        .and_then(|session_name| options.session_value(session_name, OptionName::DefaultShell))
        .or_else(|| options.global_value(OptionName::DefaultShell))
        .filter(|value| !value.is_empty())
}

#[cfg(unix)]
pub(super) fn normalize_shell_path(path: PathBuf) -> PathBuf {
    path
}

#[cfg(windows)]
pub(super) fn normalize_shell_path(path: PathBuf) -> PathBuf {
    if path.components().count() > 1 {
        return path;
    }

    find_shell_on_path(&path).unwrap_or(path)
}

#[cfg(windows)]
fn find_shell_on_path(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    if matches!(name.as_str(), "cmd" | "cmd.exe") {
        return cmd_shell_path();
    }
    if matches!(name.as_str(), "powershell" | "powershell.exe") {
        return windows_powershell_path();
    }

    search_path(path)
}

#[cfg(windows)]
fn search_path(path: &Path) -> Option<PathBuf> {
    let path_value = env::var_os("PATH")?;
    let extensions = executable_extensions(path);
    for directory in env::split_paths(&path_value) {
        for extension in &extensions {
            let candidate = directory.join(format!("{}{}", path.to_string_lossy(), extension));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(windows)]
fn executable_extensions(path: &Path) -> Vec<String> {
    if path.extension().is_some() {
        return vec![String::new()];
    }

    env::var_os("PATHEXT")
        .map(|value| {
            value
                .to_string_lossy()
                .split(';')
                .filter(|extension| !extension.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|extensions| !extensions.is_empty())
        .unwrap_or_else(|| vec![".COM".to_owned(), ".EXE".to_owned(), ".BAT".to_owned()])
}

#[cfg(unix)]
fn current_user_login_shell() -> Option<PathBuf> {
    let uid = getuid().as_raw();
    fs::read_to_string("/etc/passwd")
        .ok()?
        .lines()
        .find_map(|line| passwd_shell_for_uid(line, uid))
}

#[cfg(unix)]
fn passwd_shell_for_uid(line: &str, uid: u32) -> Option<PathBuf> {
    let mut fields = line.split(':');
    let _name = fields.next()?;
    let _password = fields.next()?;
    let parsed_uid = fields.next()?.parse::<u32>().ok()?;
    let _gid = fields.next()?;
    let _gecos = fields.next()?;
    let _home = fields.next()?;
    let shell = fields.next()?;
    (parsed_uid == uid && !shell.is_empty()).then(|| PathBuf::from(shell))
}

#[cfg(unix)]
fn default_shell_path() -> PathBuf {
    PathBuf::from("/bin/sh")
}

#[cfg(windows)]
fn default_shell_path() -> PathBuf {
    search_path(Path::new("pwsh.exe"))
        .or_else(windows_powershell_path)
        .or_else(cmd_shell_path)
        .unwrap_or_else(|| PathBuf::from("cmd.exe"))
}

#[cfg(windows)]
fn windows_powershell_path() -> Option<PathBuf> {
    env::var_os("SystemRoot").map(|root| {
        PathBuf::from(root)
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe")
    })
}

#[cfg(windows)]
fn cmd_shell_path() -> Option<PathBuf> {
    env::var_os("COMSPEC").map(PathBuf::from).or_else(|| {
        env::var_os("SystemRoot").map(|root| PathBuf::from(root).join("System32").join("cmd.exe"))
    })
}

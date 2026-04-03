use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::FileTypeExt;
use std::path::PathBuf;
use std::time::Duration;

use rmux_pty::{ChildCommand, PtyPair, Signal, TerminalSize};

#[derive(Debug, Eq, PartialEq)]
struct ProcessStat {
    pgrp: i32,
    session: i32,
    tty_nr: i32,
    tpgid: i32,
}

fn read_exact_from_master(
    master: &rmux_pty::PtyMaster,
    len: usize,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut file = File::from(master.try_clone()?.into_owned_fd());
    let mut buffer = vec![0_u8; len];
    file.read_exact(&mut buffer)?;
    Ok(buffer)
}

fn read_line_from_master(
    master: &rmux_pty::PtyMaster,
) -> Result<String, Box<dyn std::error::Error>> {
    let file = File::from(master.try_clone()?.into_owned_fd());
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    Ok(line)
}

fn pid_raw(pid: rustix::process::Pid) -> i32 {
    pid.as_raw_pid()
}

fn read_process_stat(pid: rustix::process::Pid) -> Result<ProcessStat, Box<dyn std::error::Error>> {
    let stat_path = format!("/proc/{}/stat", pid_raw(pid));
    let stat = fs::read_to_string(stat_path)?;
    let command_end = stat
        .rfind(')')
        .ok_or_else(|| "missing command terminator in /proc stat".to_string())?;
    let fields: Vec<&str> = stat[command_end + 2..].split_whitespace().collect();

    Ok(ProcessStat {
        pgrp: fields
            .get(2)
            .ok_or_else(|| "missing pgrp field in /proc stat".to_string())?
            .parse()?,
        session: fields
            .get(3)
            .ok_or_else(|| "missing session field in /proc stat".to_string())?
            .parse()?,
        tty_nr: fields
            .get(4)
            .ok_or_else(|| "missing tty_nr field in /proc stat".to_string())?
            .parse()?,
        tpgid: fields
            .get(5)
            .ok_or_else(|| "missing tpgid field in /proc stat".to_string())?
            .parse()?,
    })
}

fn child_fd_path(pid: rustix::process::Pid, fd: u8) -> PathBuf {
    format!("/proc/{}/fd/{fd}", pid_raw(pid)).into()
}

fn process_exists(pid: rustix::process::Pid) -> bool {
    fs::metadata(format!("/proc/{}", pid_raw(pid))).is_ok()
}

fn wait_for_exit(
    child: &mut rmux_pty::PtyChild,
    timeout: Duration,
) -> Result<Option<std::process::ExitStatus>, Box<dyn std::error::Error>> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }

        if std::time::Instant::now() >= deadline {
            return Ok(None);
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn allocated_pair_round_trips_resized_terminal_size() -> Result<(), Box<dyn std::error::Error>> {
    let pair = PtyPair::open_with_size(TerminalSize::new(120, 40))?;
    assert_eq!(pair.master().size()?, TerminalSize::new(120, 40));

    pair.master().resize(TerminalSize::new(200, 50))?;
    assert_eq!(pair.master().size()?, TerminalSize::new(200, 50));
    assert_eq!(pair.slave().size()?, TerminalSize::new(200, 50));

    Ok(())
}

#[test]
fn spawned_child_is_session_and_foreground_group_leader() -> Result<(), Box<dyn std::error::Error>>
{
    let mut spawned = ChildCommand::new("/bin/sh")
        .args(["-c", "sleep 30"])
        .size(TerminalSize::new(88, 28))
        .spawn()?;

    let pid = spawned.child().pid();
    let stat = read_process_stat(pid)?;
    let fd0 = fs::read_link(child_fd_path(pid, 0))?;
    let fd0_metadata = fs::metadata(child_fd_path(pid, 0))?;

    assert_eq!(stat.session, pid_raw(pid));
    assert_eq!(stat.pgrp, pid_raw(pid));
    assert_eq!(stat.tpgid, pid_raw(pid));
    assert_ne!(stat.tty_nr, 0);
    assert!(fd0.starts_with("/dev/pts/"));
    assert!(fd0_metadata.file_type().is_char_device());

    spawned.child().kill(Signal::KILL)?;
    let status = spawned.child_mut().wait()?;
    assert!(!status.success());

    Ok(())
}

#[test]
fn spawned_child_reads_and_writes_through_master() -> Result<(), Box<dyn std::error::Error>> {
    let mut spawned = ChildCommand::new("/bin/sh")
        .args(["-c", "stty raw -echo; printf READY; exec cat"])
        .size(TerminalSize::new(90, 30))
        .spawn()?;

    assert_eq!(read_exact_from_master(spawned.master(), 5)?, b"READY");

    let payload = b"hello over pty";
    let mut writer = File::from(spawned.master().try_clone()?.into_owned_fd());
    writer.write_all(payload)?;
    writer.flush()?;

    assert_eq!(
        read_exact_from_master(spawned.master(), payload.len())?,
        payload
    );
    spawned.master().resize(TerminalSize::new(101, 41))?;
    assert_eq!(spawned.master().size()?, TerminalSize::new(101, 41));

    spawned.child().kill(Signal::TERM)?;
    let status = spawned.child_mut().wait()?;
    assert!(!status.success());

    Ok(())
}

#[test]
fn kill_terminates_the_pty_process_group() -> Result<(), Box<dyn std::error::Error>> {
    let mut spawned = ChildCommand::new("/bin/sh")
        .args([
            "-c",
            "stty raw -echo; sleep 30 & printf '%s\\n' \"$!\"; wait",
        ])
        .size(TerminalSize::new(80, 24))
        .spawn()?;

    let descendant_pid = read_line_from_master(spawned.master())?
        .trim()
        .parse::<i32>()?;
    let descendant_pid = rustix::process::Pid::from_raw(descendant_pid)
        .ok_or_else(|| "helper shell returned pid 0".to_string())?;
    let descendant_stat = read_process_stat(descendant_pid)?;

    assert_eq!(descendant_stat.pgrp, pid_raw(spawned.child().pid()));

    spawned.child().kill(Signal::TERM)?;
    let deadline = std::time::Instant::now() + Duration::from_millis(250);
    while process_exists(descendant_pid) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }

    if process_exists(descendant_pid) {
        let _ = spawned.child().kill(Signal::KILL);
        let _ = spawned.child_mut().wait();
    }

    assert!(!process_exists(descendant_pid), "descendant survived TERM");

    let status = wait_for_exit(spawned.child_mut(), Duration::from_millis(250))?;
    if status.is_none() {
        let _ = spawned.child().kill(Signal::KILL);
        let _ = spawned.child_mut().wait();
    }
    assert!(status.is_some(), "PTY leader did not exit after TERM");

    Ok(())
}

#[test]
fn spawned_child_receives_environment_and_reaps_cleanly() -> Result<(), Box<dyn std::error::Error>>
{
    let mut spawned = ChildCommand::new("/bin/sh")
        .args(["-c", "printf %s \"$RMUX_TEST_VALUE\""])
        .env("RMUX_TEST_VALUE", "visible")
        .size(TerminalSize::new(80, 24))
        .spawn()?;

    assert_eq!(read_exact_from_master(spawned.master(), 7)?, b"visible");
    let status = spawned.child_mut().wait()?;
    assert!(status.success());
    assert!(spawned.child_mut().try_wait()?.is_some());

    Ok(())
}

#[test]
fn wait_reports_child_exit_status() -> Result<(), Box<dyn std::error::Error>> {
    let mut spawned = ChildCommand::new("/bin/sh")
        .args(["-c", "exit 7"])
        .size(TerminalSize::new(80, 24))
        .spawn()?;

    let status = spawned.child_mut().wait()?;
    assert_eq!(status.code(), Some(7));

    std::thread::sleep(Duration::from_millis(10));
    assert!(spawned.child_mut().try_wait()?.is_some());

    Ok(())
}

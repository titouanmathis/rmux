#![cfg(windows)]

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use rmux_pty::{ChildCommand, PtyMaster, PtyPair, TerminalSize};

#[path = "windows_conpty/job.rs"]
mod job;

#[test]
fn conpty_pair_opens_resizes_and_clones_master() -> Result<(), Box<dyn std::error::Error>> {
    let pair = PtyPair::open_with_size(TerminalSize::new(100, 30))?;
    assert_eq!(pair.master().size()?, TerminalSize::new(100, 30));

    pair.master().resize(TerminalSize::new(120, 40))?;
    assert_eq!(pair.master().size()?, TerminalSize::new(120, 40));

    let clone = pair.master().try_clone()?;
    assert_eq!(clone.size()?, TerminalSize::new(120, 40));
    Ok(())
}

#[test]
fn conpty_spawn_reads_child_output_and_waits() -> Result<(), Box<dyn std::error::Error>> {
    let mut spawned = ChildCommand::new("C:\\Windows\\System32\\cmd.exe")
        .args(["/C", "echo RMUX_SPAWN_OK"])
        .size(TerminalSize::new(100, 30))
        .spawn()?;

    let output = read_until(spawned.master(), b"RMUX_SPAWN_OK", Duration::from_secs(2))?;
    let status = spawned.child_mut().wait()?;

    assert!(status.success());
    assert!(
        String::from_utf8_lossy(&output).contains("RMUX_SPAWN_OK"),
        "expected marker in ConPTY output, got {:?}",
        String::from_utf8_lossy(&output)
    );
    assert!(spawned.child_mut().try_wait()?.is_some());
    Ok(())
}

#[test]
fn conpty_interactive_cmd_accepts_written_input() -> Result<(), Box<dyn std::error::Error>> {
    let mut spawned = ChildCommand::new("C:\\Windows\\System32\\cmd.exe")
        .args(["/D", "/K"])
        .size(TerminalSize::new(100, 30))
        .spawn()?;

    let io = spawned.master().try_clone_io()?;
    let mut output = read_until_io(&io, b">", Duration::from_secs(2))?;
    io.write_all(b"echo RMUX_INTERACTIVE_OK\r\n")?;
    output.extend(read_until_io(
        &io,
        b"RMUX_INTERACTIVE_OK",
        Duration::from_secs(2),
    )?);

    spawned.child().terminate_forcefully()?;
    let _ = spawned.child_mut().wait()?;

    assert!(
        String::from_utf8_lossy(&output).contains("RMUX_INTERACTIVE_OK"),
        "expected interactive marker in ConPTY output, got {:?}",
        String::from_utf8_lossy(&output)
    );
    Ok(())
}

#[test]
fn conpty_background_reader_receives_output_after_input() -> Result<(), Box<dyn std::error::Error>>
{
    let mut spawned = ChildCommand::new("C:\\Windows\\System32\\cmd.exe")
        .args(["/D", "/K"])
        .size(TerminalSize::new(100, 30))
        .spawn()?;

    let reader = spawned.master().try_clone_io()?;
    let writer = spawned.master().try_clone_io()?;
    let (tx, rx) = mpsc::channel();
    let reader_thread = thread::spawn(move || {
        let result = read_until_io(&reader, b"RMUX_BACKGROUND_OK", Duration::from_secs(4));
        let _ = tx.send(
            result
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .map_err(|error| error.to_string()),
        );
    });

    thread::sleep(Duration::from_millis(100));
    writer.write_all(b"echo RMUX_BACKGROUND_OK\r\n")?;

    let output = rx
        .recv_timeout(Duration::from_secs(5))?
        .map_err(std::io::Error::other)?;
    spawned.child().terminate_forcefully()?;
    let _ = spawned.child_mut().wait()?;
    reader_thread
        .join()
        .map_err(|_| "background reader thread panicked")?;

    assert!(
        output.contains("RMUX_BACKGROUND_OK"),
        "expected background reader marker in ConPTY output, got {output:?}"
    );
    Ok(())
}

#[test]
fn conpty_spawn_succeeds_when_parent_is_already_in_job() -> Result<(), Box<dyn std::error::Error>> {
    job::run_parent_job_helper(job::ParentJobMode::NoBreakaway)
}

#[test]
fn conpty_breakaway_retry_succeeds_when_parent_job_allows_breakaway(
) -> Result<(), Box<dyn std::error::Error>> {
    job::run_parent_job_helper(job::ParentJobMode::BreakawayAllowed)
}

#[test]
fn conpty_spawn_inside_parent_job_helper() -> Result<(), Box<dyn std::error::Error>> {
    let Some(mode) = job::requested_helper_mode() else {
        return Ok(());
    };
    let _parent_job = job::assign_current_process_to_job(mode)?;
    let mut spawned = ChildCommand::new("C:\\Windows\\System32\\cmd.exe")
        .args(["/C", "echo RMUX_PARENT_JOB_OK & ping -n 30 127.0.0.1 >NUL"])
        .size(TerminalSize::new(80, 24))
        .spawn()?;

    let output = read_until(
        spawned.master(),
        b"RMUX_PARENT_JOB_OK",
        Duration::from_secs(2),
    )?;
    assert!(
        String::from_utf8_lossy(&output).contains("RMUX_PARENT_JOB_OK"),
        "expected parent-job marker in ConPTY output, got {:?}",
        String::from_utf8_lossy(&output)
    );

    spawned.child().terminate_forcefully()?;
    let status = spawned.child_mut().wait()?;
    assert!(!status.success());
    assert!(spawned.child_mut().try_wait()?.is_some());
    Ok(())
}

#[test]
fn conpty_force_kill_reaps_child() -> Result<(), Box<dyn std::error::Error>> {
    let mut spawned = ChildCommand::new("C:\\Windows\\System32\\cmd.exe")
        .args(["/C", "ping -n 30 127.0.0.1 >NUL"])
        .size(TerminalSize::new(80, 24))
        .spawn()?;

    spawned.child().terminate_forcefully()?;
    let status = spawned.child_mut().wait()?;
    assert!(!status.success());
    assert!(spawned.child_mut().try_wait()?.is_some());
    Ok(())
}

#[test]
fn conpty_resize_after_child_exit_is_not_fatal() -> Result<(), Box<dyn std::error::Error>> {
    let mut spawned = ChildCommand::new("C:\\Windows\\System32\\cmd.exe")
        .args(["/C", "exit 0"])
        .size(TerminalSize::new(80, 24))
        .spawn()?;

    assert!(spawned.child_mut().wait()?.success());
    spawned.master().resize(TerminalSize::new(90, 25))?;
    Ok(())
}

fn read_until(
    master: &PtyMaster,
    needle: &[u8],
    timeout: Duration,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let io = master.try_clone_io()?;
    read_until_io(&io, needle, timeout)
}

fn read_until_io(
    io: &rmux_pty::PtyIo,
    needle: &[u8],
    timeout: Duration,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + timeout;
    let mut output = Vec::new();
    let mut buffer = [0_u8; 4096];

    while Instant::now() < deadline {
        let bytes_read = io.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        output.extend_from_slice(&buffer[..bytes_read]);
        if output.windows(needle.len()).any(|window| window == needle) {
            return Ok(output);
        }
    }

    Ok(output)
}

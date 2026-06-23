use super::parse::{parse_file_change_line, parse_preview_change_line};
use super::{
    command::{OneDriveCommandKind, add_single_directory_scope, build_command},
    output::{
        classify_onedrive_error, combined_output, is_auth_required, parse_confirmation,
        parse_version,
    },
};
use crate::{
    event::{BackendError, BackendEvent, ClientCheck, OperationOutcome, ProcPhase, Version},
    profile::config::ensure_transfer_metrics_enabled,
    profile::{Account, auth_response_path, auth_url_path, is_authenticated},
};
use std::{
    fs,
    io::{self, BufReader, Read},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

const MIN_ONEDRIVE_VERSION: Version = Version {
    major: 2,
    minor: 5,
    patch: 0,
};

#[derive(Clone)]
pub struct SyncHandle {
    child: Arc<Mutex<Child>>,
    stop_requested: Arc<AtomicBool>,
}

#[derive(Clone, Copy)]
enum OutputMode {
    Live,
    Preview,
}

pub fn check_client(binary: String, sender: mpsc::Sender<BackendEvent>) {
    thread::spawn(move || {
        let result = match Command::new(&binary).arg("--version").output() {
            Ok(output) => {
                let mut combined = String::from_utf8_lossy(&output.stdout).to_string();
                combined.push_str(&String::from_utf8_lossy(&output.stderr));
                match parse_version(&combined) {
                    Some(version) if version >= MIN_ONEDRIVE_VERSION => ClientCheck::Ready(version),
                    Some(version) => ClientCheck::Unsupported {
                        found: version,
                        minimum: MIN_ONEDRIVE_VERSION,
                    },
                    None => {
                        ClientCheck::Missing("unable to determine onedrive version".to_string())
                    }
                }
            }
            Err(error) => ClientCheck::Missing(format!("{binary}: {error}")),
        };
        let _ = sender.send(BackendEvent::ClientChecked(result));
    });
}

pub fn start_authentication(
    account: Account,
    binary: String,
    sender: mpsc::Sender<BackendEvent>,
) -> io::Result<SyncHandle> {
    let auth_url = auth_url_path(&account);
    let auth_response = auth_response_path(&account);
    let _ = fs::remove_file(&auth_url);
    let _ = fs::remove_file(&auth_response);

    let child = Command::new(&binary)
        .arg("--confdir")
        .arg(&account.config_dir)
        .arg("--auth-files")
        .arg(format!(
            "{}:{}",
            auth_url.display(),
            auth_response.display()
        ))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let child = Arc::new(Mutex::new(child));
    let stop_requested = Arc::new(AtomicBool::new(false));
    let wait_child = Arc::clone(&child);
    let wait_stop_requested = Arc::clone(&stop_requested);

    thread::spawn(move || {
        for _ in 0..300 {
            if wait_stop_requested.load(Ordering::SeqCst) {
                let mut child = wait_child.lock().ok();
                if let Some(ref mut child) = child {
                    let _ = child.kill();
                }
                let _ = sender.send(BackendEvent::AuthFinished {
                    account_id: account.id.clone(),
                    success: false,
                    error: Some(BackendError::WaitFailed(
                        ProcPhase::Auth,
                        "requested stop".to_string(),
                    )),
                });
                return;
            }
            if let Ok(url) = fs::read_to_string(&auth_url) {
                let trimmed = url.trim();
                if !trimmed.is_empty() {
                    let _ = sender.send(BackendEvent::AuthUrl {
                        account_id: account.id.clone(),
                        url: trimmed.to_string(),
                    });
                    break;
                }
            }
            thread::sleep(Duration::from_millis(200));
        }

        loop {
            if wait_stop_requested.load(Ordering::SeqCst) {
                let mut child = wait_child.lock().ok();
                if let Some(ref mut child) = child {
                    let _ = child.kill();
                }
                let _ = sender.send(BackendEvent::AuthFinished {
                    account_id: account.id.clone(),
                    success: false,
                    error: Some(BackendError::WaitFailed(
                        ProcPhase::Auth,
                        "requested stop".to_string(),
                    )),
                });
                return;
            }
            let result = wait_child
                .lock()
                .ok()
                .and_then(|mut child| child.try_wait().ok());
            match result {
                Some(Some(status)) => {
                    let success = status.success() || is_authenticated(&account);
                    let error = (!success).then(|| classify_onedrive_error(""));
                    let _ = sender.send(BackendEvent::AuthFinished {
                        account_id: account.id,
                        success,
                        error,
                    });
                    return;
                }
                None => {
                    let _ = sender.send(BackendEvent::AuthFinished {
                        account_id: account.id,
                        success: false,
                        error: Some(BackendError::MonitorInaccessible),
                    });
                    return;
                }
                Some(None) => thread::sleep(Duration::from_millis(500)),
            }
        }
    });

    Ok(SyncHandle {
        child,
        stop_requested,
    })
}

struct StreamedChild {
    child: Arc<Mutex<Child>>,
    stop_requested: Arc<AtomicBool>,
    output: Arc<Mutex<String>>,
}

fn spawn_streamed_child(
    account: &Account,
    binary: String,
    kind: OneDriveCommandKind,
    output_mode: OutputMode,
    sender: &mpsc::Sender<BackendEvent>,
) -> io::Result<StreamedChild> {
    ensure_transfer_metrics_enabled(&account.config_dir)?;

    let mut child = build_command(binary, account, kind)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let output = attach_readers(&account.id, &mut child, sender, output_mode);
    let child = Arc::new(Mutex::new(child));
    let stop_requested = Arc::new(AtomicBool::new(false));
    Ok(StreamedChild {
        child,
        stop_requested,
        output,
    })
}

#[derive(Clone, Copy)]
enum WaiterKind {
    Preview,
    Sync,
}

impl WaiterKind {
    fn phase(self) -> ProcPhase {
        match self {
            WaiterKind::Preview => ProcPhase::Preview,
            WaiterKind::Sync => ProcPhase::Sync,
        }
    }
}

fn spawn_outcome_waiter(
    streamed: StreamedChild,
    account_id: String,
    sender: mpsc::Sender<BackendEvent>,
    kind: WaiterKind,
) -> SyncHandle {
    let StreamedChild {
        child,
        stop_requested,
        output,
    } = streamed;
    let wait_child = Arc::clone(&child);
    let wait_stop_requested = Arc::clone(&stop_requested);

    thread::spawn(move || {
        let result = wait_for_child(&wait_child);
        let requested_stop = wait_stop_requested.load(Ordering::SeqCst);
        let combined = output
            .lock()
            .map(|output| output.clone())
            .unwrap_or_default();
        let outcome = match result {
            Ok(success) => OperationOutcome {
                success,
                requested_stop,
                auth_required: !success && is_auth_required(&combined),
                error: (!success && !requested_stop).then(|| classify_onedrive_error(&combined)),
                requires_confirmation: parse_confirmation(&combined),
            },
            Err(error) => OperationOutcome {
                success: false,
                requested_stop,
                auth_required: false,
                error: Some(BackendError::WaitFailed(kind.phase(), error.to_string())),
                requires_confirmation: None,
            },
        };
        let event = match kind {
            WaiterKind::Preview => BackendEvent::PreviewFinished {
                account_id,
                outcome,
            },
            WaiterKind::Sync => BackendEvent::SyncFinished {
                account_id,
                outcome,
            },
        };
        let _ = sender.send(event);
    });

    SyncHandle {
        child,
        stop_requested,
    }
}

pub fn start_preview(
    account: Account,
    binary: String,
    sender: mpsc::Sender<BackendEvent>,
) -> io::Result<SyncHandle> {
    let streamed = spawn_streamed_child(
        &account,
        binary,
        OneDriveCommandKind::Preview,
        OutputMode::Preview,
        &sender,
    )?;
    Ok(spawn_outcome_waiter(
        streamed,
        account.id,
        sender,
        WaiterKind::Preview,
    ))
}

pub fn start_sync(
    account: Account,
    binary: String,
    sender: mpsc::Sender<BackendEvent>,
    force: bool,
    resync: bool,
) -> io::Result<SyncHandle> {
    let streamed = spawn_streamed_child(
        &account,
        binary,
        OneDriveCommandKind::Sync { force, resync },
        OutputMode::Live,
        &sender,
    )?;
    Ok(spawn_outcome_waiter(
        streamed,
        account.id,
        sender,
        WaiterKind::Sync,
    ))
}

pub fn start_monitor(
    account: Account,
    binary: String,
    sender: mpsc::Sender<BackendEvent>,
) -> io::Result<SyncHandle> {
    let streamed = spawn_streamed_child(
        &account,
        binary,
        OneDriveCommandKind::Monitor,
        OutputMode::Live,
        &sender,
    )?;
    let StreamedChild {
        child,
        stop_requested,
        output,
    } = streamed;
    let wait_child = Arc::clone(&child);
    let wait_stop_requested = Arc::clone(&stop_requested);
    let account_id = account.id;

    thread::spawn(move || {
        let success = loop {
            let result = {
                match wait_child.lock() {
                    Ok(mut child) => child.try_wait(),
                    Err(_) => {
                        let _ = sender.send(BackendEvent::MonitorStopped {
                            account_id: account_id.clone(),
                            outcome: OperationOutcome {
                                success: false,
                                requested_stop: wait_stop_requested.load(Ordering::SeqCst),
                                auth_required: false,
                                error: Some(BackendError::MonitorInaccessible),
                                requires_confirmation: None,
                            },
                        });
                        return;
                    }
                }
            };

            match result {
                Ok(Some(status)) => break status.success(),
                Ok(None) => thread::sleep(Duration::from_millis(500)),
                Err(error) => {
                    let _ = sender.send(BackendEvent::MonitorStopped {
                        account_id: account_id.clone(),
                        outcome: OperationOutcome {
                            success: false,
                            requested_stop: wait_stop_requested.load(Ordering::SeqCst),
                            auth_required: false,
                            error: Some(BackendError::MonitorPollFailed(error.to_string())),
                            requires_confirmation: None,
                        },
                    });
                    return;
                }
            }
        };

        let combined = output
            .lock()
            .map(|output| output.clone())
            .unwrap_or_default();
        let _ = sender.send(BackendEvent::MonitorStopped {
            account_id,
            outcome: OperationOutcome {
                success,
                requested_stop: wait_stop_requested.load(Ordering::SeqCst),
                auth_required: !success && is_auth_required(&combined),
                error: (!success).then(|| classify_onedrive_error(&combined)),
                requires_confirmation: parse_confirmation(&combined),
            },
        });
    });

    Ok(SyncHandle {
        child,
        stop_requested,
    })
}

pub fn reconcile_preview_change(
    account: &Account,
    binary: String,
    path: &str,
) -> io::Result<String> {
    ensure_transfer_metrics_enabled(&account.config_dir)?;

    let mut command = build_command(binary, account, OneDriveCommandKind::ReconcileSync);
    add_single_directory_scope(&mut command, path);
    run_reconcile_command(command)
}

pub fn display_reconcile_status(
    account: &Account,
    binary: String,
    path: &str,
) -> io::Result<String> {
    let mut command = build_command(binary, account, OneDriveCommandKind::DisplaySyncStatus);
    add_single_directory_scope(&mut command, path);
    run_reconcile_command(command)
}

fn run_reconcile_command(mut command: Command) -> io::Result<String> {
    let output = command.output()?;
    let combined = combined_output(&output.stdout, &output.stderr);

    if output.status.success() {
        Ok(combined)
    } else {
        Err(io::Error::other(combined.trim().to_string()))
    }
}

pub fn stop_handle(handle: &SyncHandle) -> io::Result<()> {
    handle.stop_requested.store(true, Ordering::SeqCst);
    let mut child = handle
        .child
        .lock()
        .map_err(|_| io::Error::other("failed to lock onedrive process"))?;
    terminate_child(&mut child)
}

fn wait_for_child(child: &Arc<Mutex<Child>>) -> io::Result<bool> {
    loop {
        let result = {
            let mut child = child
                .lock()
                .map_err(|_| io::Error::other("failed to lock onedrive process"))?;
            child.try_wait()
        };
        match result? {
            Some(status) => return Ok(status.success()),
            None => thread::sleep(Duration::from_millis(500)),
        }
    }
}

fn terminate_child(child: &mut Child) -> io::Result<()> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }

    #[cfg(unix)]
    {
        let pid = child.id().to_string();
        let _ = Command::new("kill").arg("-TERM").arg(pid).status();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if child.try_wait()?.is_some() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    child.kill()?;
    let _ = child.wait();
    Ok(())
}

fn attach_readers(
    account_id: &str,
    child: &mut Child,
    sender: &mpsc::Sender<BackendEvent>,
    mode: OutputMode,
) -> Arc<Mutex<String>> {
    let output = Arc::new(Mutex::new(String::new()));
    if let Some(stdout) = child.stdout.take() {
        spawn_transfer_reader(
            account_id.to_string(),
            stdout,
            sender.clone(),
            Arc::clone(&output),
            mode,
        );
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_transfer_reader(
            account_id.to_string(),
            stderr,
            sender.clone(),
            Arc::clone(&output),
            mode,
        );
    }
    output
}

fn spawn_transfer_reader<R>(
    account_id: String,
    reader: R,
    sender: mpsc::Sender<BackendEvent>,
    output: Arc<Mutex<String>>,
    mode: OutputMode,
) where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut buffer = Vec::new();
        let mut byte = [0_u8; 1];
        loop {
            match reader.read(&mut byte) {
                Ok(0) => break,
                Ok(_) => {
                    if byte[0] == b'\n' || byte[0] == b'\r' {
                        send_transfer_chunk(&account_id, &buffer, &sender, &output, mode);
                        buffer.clear();
                    } else {
                        buffer.push(byte[0]);
                    }
                }
                Err(_) => break,
            }
        }
        if !buffer.is_empty() {
            send_transfer_chunk(&account_id, &buffer, &sender, &output, mode);
        }
    });
}

fn send_transfer_chunk(
    account_id: &str,
    chunk: &[u8],
    sender: &mpsc::Sender<BackendEvent>,
    output: &Arc<Mutex<String>>,
    mode: OutputMode,
) {
    let line = String::from_utf8_lossy(chunk);
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    if let Ok(mut output) = output.lock() {
        output.push_str(line);
        output.push('\n');
    }
    match mode {
        OutputMode::Live => {
            if let Some(file) = parse_file_change_line(line) {
                let _ = sender.send(BackendEvent::TransferEvent {
                    account_id: account_id.to_string(),
                    file,
                });
            }
        }
        OutputMode::Preview => {
            if let Some(change) = parse_preview_change_line(line) {
                let _ = sender.send(BackendEvent::PreviewEvent {
                    account_id: account_id.to_string(),
                    change,
                });
            }
        }
    }
    if let Some(kind) = parse_confirmation(line) {
        let _ = sender.send(BackendEvent::ConfirmationRequired {
            account_id: account_id.to_string(),
            kind,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::test_support::{fake_onedrive_binary, sync_test_fixture, temp_account};

    #[cfg(unix)]
    fn fixture_account(id: &str, name: &str, config_dir: impl AsRef<std::path::Path>) -> Account {
        use crate::profile::AccountStatus;
        Account {
            id: id.to_string(),
            name: name.to_string(),
            email: String::new(),
            config_dir: config_dir.as_ref().to_string_lossy().to_string(),
            sync_dir: "~/OneDrive".to_string(),
            status: AccountStatus::Authenticated,
        }
    }

    #[cfg(unix)]
    #[test]
    fn sync_handle_can_stop_running_sync() {
        let (binary, config_dir, root) =
            sync_test_fixture("stop", "trap 'exit 0' TERM\nwhile true; do sleep 1; done");
        let account = fixture_account("sync-stop", "Sync Stop", config_dir);
        let (sender, receiver) = std::sync::mpsc::channel();
        let handle = start_sync(
            account,
            binary.to_string_lossy().to_string(),
            sender,
            false,
            false,
        )
        .unwrap();

        stop_handle(&handle).unwrap();
        let event = receiver.recv_timeout(Duration::from_secs(3)).unwrap();
        match event {
            BackendEvent::SyncFinished { outcome, .. } => {
                assert!(outcome.requested_stop);
                assert!(!outcome.auth_required);
            }
            other => panic!("expected SyncFinished, got {other:?}"),
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn forced_sync_passes_force_flag_to_onedrive() {
        let (binary, config_dir, root) = sync_test_fixture("force", "printf '%s\\n' \"$@\" > args");
        let account = fixture_account("force-sync", "Force Sync", config_dir);
        let (sender, receiver) = std::sync::mpsc::channel();
        let _handle = start_sync(
            account,
            binary.to_string_lossy().to_string(),
            sender,
            true,
            false,
        )
        .unwrap();
        let event = receiver.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(matches!(
            event,
            BackendEvent::SyncFinished {
                outcome: OperationOutcome {
                    success: true,
                    requested_stop: false,
                    ..
                },
                ..
            }
        ));

        let args = fs::read_to_string(root.join("args")).unwrap();
        assert!(args.lines().any(|arg| arg == "--force"));
        assert!(!args.lines().any(|arg| arg == "--resync-auth"));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn resync_passes_resync_flag_to_onedrive() {
        let (binary, config_dir, root) =
            sync_test_fixture("resync", "printf '%s\\n' \"$@\" > args");
        let account = fixture_account("resync", "Resync", config_dir);
        let (sender, receiver) = std::sync::mpsc::channel();
        let _handle = start_sync(
            account,
            binary.to_string_lossy().to_string(),
            sender,
            false,
            true,
        )
        .unwrap();
        let event = receiver.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(matches!(
            event,
            BackendEvent::SyncFinished {
                outcome: OperationOutcome {
                    success: true,
                    requested_stop: false,
                    ..
                },
                ..
            }
        ));

        let args = fs::read_to_string(root.join("args")).unwrap();
        assert!(args.lines().any(|arg| arg == "--resync"));
        assert!(args.lines().any(|arg| arg == "--resync-auth"));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn preview_sync_passes_dry_run_flag_to_onedrive() {
        let (binary, config_dir, root) = sync_test_fixture(
            "preview",
            "printf '%s\\n' \"$@\" > args\nprintf '%s\\n' 'Uploading new file: ./docs/a.txt ... done'",
        );
        let account = fixture_account("preview-sync", "Preview Sync", config_dir);
        let (sender, receiver) = std::sync::mpsc::channel();
        let _handle = start_preview(account, binary.to_string_lossy().to_string(), sender).unwrap();

        let events = [
            receiver.recv_timeout(Duration::from_secs(3)).unwrap(),
            receiver.recv_timeout(Duration::from_secs(3)).unwrap(),
        ];
        assert!(
            events
                .iter()
                .any(|event| matches!(event, BackendEvent::PreviewEvent { .. }))
        );
        assert!(events.iter().any(|event| matches!(
            event,
            BackendEvent::PreviewFinished {
                outcome: OperationOutcome {
                    success: true,
                    requested_stop: false,
                    ..
                },
                ..
            }
        )));

        let args = fs::read_to_string(root.join("args")).unwrap();
        assert!(args.lines().any(|arg| arg == "--sync"));
        assert!(args.lines().any(|arg| arg == "--verbose"));
        assert!(args.lines().any(|arg| arg == "--local-first"));
        assert!(args.lines().any(|arg| arg == "--dry-run"));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn preview_handle_reports_requested_stop() {
        let (binary, config_dir, root) = sync_test_fixture(
            "preview-stop",
            "trap 'exit 0' TERM\nwhile true; do sleep 1; done",
        );
        let account = fixture_account("preview-stop", "Preview Stop", config_dir);
        let (sender, receiver) = std::sync::mpsc::channel();
        let handle = start_preview(account, binary.to_string_lossy().to_string(), sender).unwrap();

        stop_handle(&handle).unwrap();
        let event = receiver.recv_timeout(Duration::from_secs(3)).unwrap();
        match event {
            BackendEvent::PreviewFinished { outcome, .. } => {
                assert!(outcome.requested_stop);
                assert!(!outcome.auth_required);
            }
            other => panic!("expected PreviewFinished, got {other:?}"),
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn reconcile_preview_change_uses_single_directory_parent_scope() {
        let (binary, output_path) =
            fake_onedrive_binary("Sync with Microsoft OneDrive is complete\n", 0);
        let account = temp_account("proc");

        reconcile_preview_change(&account, binary.to_string_lossy().to_string(), "docs/a.txt")
            .expect("reconcile should succeed");

        let args = fs::read_to_string(output_path).expect("args should be captured");
        assert!(args.lines().any(|arg| arg == "--sync"));
        assert!(args.lines().any(|arg| arg == "--verbose"));
        assert!(args.lines().any(|arg| arg == "--single-directory"));
        assert!(args.lines().any(|arg| arg == "docs"));
    }

    #[cfg(unix)]
    #[test]
    fn display_reconcile_status_uses_single_directory_parent_scope() {
        let (binary, output_path) = fake_onedrive_binary("The directory is in sync\n", 0);
        let account = temp_account("proc");

        display_reconcile_status(&account, binary.to_string_lossy().to_string(), "docs/a.txt")
            .expect("status should succeed");

        let args = fs::read_to_string(output_path).expect("args should be captured");
        assert!(args.lines().any(|arg| arg == "--display-sync-status"));
        assert!(args.lines().any(|arg| arg == "--single-directory"));
        assert!(args.lines().any(|arg| arg == "docs"));
    }
}

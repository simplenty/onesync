use super::parse::{parse_file_change_line, parse_preview_change_line};
use super::{
    command::{OneDriveCommandKind, add_single_directory_scope, build_command},
    output::{
        classify_onedrive_error, combined_output, is_auth_required, parse_confirmation,
        parse_version,
    },
};
use crate::{
    event::{BackendError, BackendEvent, ClientCheck, ProcPhase, Version},
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

pub type MonitorHandle = SyncHandle;

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
            match fs::read_to_string(&auth_url) {
                Ok(url) => {
                    let trimmed = url.trim();
                    if !trimmed.is_empty() {
                        let _ = sender.send(BackendEvent::AuthUrl {
                            account_id: account.id.clone(),
                            url: trimmed.to_string(),
                        });
                        break;
                    }
                }
                Err(_) => {}
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

pub fn start_sync(
    account: Account,
    binary: String,
    sender: mpsc::Sender<BackendEvent>,
) -> io::Result<SyncHandle> {
    start_sync_with_options(account, binary, sender, false, false)
}

pub fn start_forced_sync(
    account: Account,
    binary: String,
    sender: mpsc::Sender<BackendEvent>,
) -> io::Result<SyncHandle> {
    start_sync_with_options(account, binary, sender, true, false)
}

pub fn start_resync(
    account: Account,
    binary: String,
    sender: mpsc::Sender<BackendEvent>,
) -> io::Result<SyncHandle> {
    start_sync_with_options(account, binary, sender, false, true)
}

pub fn start_preview(
    account: Account,
    binary: String,
    sender: mpsc::Sender<BackendEvent>,
) -> io::Result<SyncHandle> {
    ensure_transfer_metrics_enabled(&account.config_dir)?;

    let mut child = build_command(binary, &account, OneDriveCommandKind::Preview)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let output = attach_readers(&account.id, &mut child, &sender, OutputMode::Preview);
    let child = Arc::new(Mutex::new(child));
    let stop_requested = Arc::new(AtomicBool::new(false));
    let wait_child = Arc::clone(&child);
    let wait_stop_requested = Arc::clone(&stop_requested);

    thread::spawn(move || {
        let result = wait_for_child(&wait_child);
        let requested_stop = wait_stop_requested.load(Ordering::SeqCst);
        let combined = output
            .lock()
            .map(|output| output.clone())
            .unwrap_or_default();
        match result {
            Ok(success) => {
                let _ = sender.send(BackendEvent::PreviewFinished {
                    account_id: account.id,
                    success,
                    requested_stop,
                    auth_required: !success && is_auth_required(&combined),
                    error: (!success && !requested_stop)
                        .then(|| classify_onedrive_error(&combined)),
                    requires_confirmation: parse_confirmation(&combined),
                });
            }
            Err(error) => {
                let _ = sender.send(BackendEvent::PreviewFinished {
                    account_id: account.id,
                    success: false,
                    requested_stop,
                    auth_required: false,
                    error: Some(BackendError::WaitFailed(
                        ProcPhase::Preview,
                        error.to_string(),
                    )),
                    requires_confirmation: None,
                });
            }
        }
    });

    Ok(SyncHandle {
        child,
        stop_requested,
    })
}

fn start_sync_with_options(
    account: Account,
    binary: String,
    sender: mpsc::Sender<BackendEvent>,
    force: bool,
    resync: bool,
) -> io::Result<SyncHandle> {
    ensure_transfer_metrics_enabled(&account.config_dir)?;

    let mut child = build_command(
        binary,
        &account,
        OneDriveCommandKind::Sync { force, resync },
    )
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()?;

    let output = attach_readers(&account.id, &mut child, &sender, OutputMode::Live);
    let child = Arc::new(Mutex::new(child));
    let stop_requested = Arc::new(AtomicBool::new(false));
    let wait_child = Arc::clone(&child);
    let wait_stop_requested = Arc::clone(&stop_requested);

    thread::spawn(move || {
        let result = wait_for_child(&wait_child);
        let requested_stop = wait_stop_requested.load(Ordering::SeqCst);
        let combined = output
            .lock()
            .map(|output| output.clone())
            .unwrap_or_default();

        match result {
            Ok(success) => {
                let auth_required = !success && is_auth_required(&combined);
                let _ = sender.send(BackendEvent::SyncFinished {
                    account_id: account.id,
                    success,
                    requested_stop,
                    auth_required,
                    error: (!success && !requested_stop)
                        .then(|| classify_onedrive_error(&combined)),
                    requires_confirmation: parse_confirmation(&combined),
                });
            }
            Err(error) => {
                let _ = sender.send(BackendEvent::SyncFinished {
                    account_id: account.id,
                    success: false,
                    requested_stop,
                    auth_required: false,
                    error: Some(BackendError::WaitFailed(ProcPhase::Sync, error.to_string())),
                    requires_confirmation: None,
                });
            }
        }
    });

    Ok(SyncHandle {
        child,
        stop_requested,
    })
}

pub fn start_monitor(
    account: Account,
    binary: String,
    sender: mpsc::Sender<BackendEvent>,
) -> io::Result<MonitorHandle> {
    ensure_transfer_metrics_enabled(&account.config_dir)?;

    let mut child = build_command(binary, &account, OneDriveCommandKind::Monitor)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let output = attach_readers(&account.id, &mut child, &sender, OutputMode::Live);
    let child = Arc::new(Mutex::new(child));
    let stop_requested = Arc::new(AtomicBool::new(false));
    let wait_child = Arc::clone(&child);
    let wait_stop_requested = Arc::clone(&stop_requested);

    thread::spawn(move || {
        let success = loop {
            let result = {
                match wait_child.lock() {
                    Ok(mut child) => child.try_wait(),
                    Err(_) => {
                        let _ = sender.send(BackendEvent::MonitorStopped {
                            account_id: account.id,
                            success: false,
                            requested_stop: wait_stop_requested.load(Ordering::SeqCst),
                            auth_required: false,
                            error: Some(BackendError::MonitorInaccessible),
                            requires_confirmation: None,
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
                        account_id: account.id,
                        success: false,
                        requested_stop: wait_stop_requested.load(Ordering::SeqCst),
                        auth_required: false,
                        error: Some(BackendError::MonitorPollFailed(error.to_string())),
                        requires_confirmation: None,
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
            account_id: account.id,
            success,
            requested_stop: wait_stop_requested.load(Ordering::SeqCst),
            auth_required: !success && is_auth_required(&combined),
            error: (!success).then(|| classify_onedrive_error(&combined)),
            requires_confirmation: parse_confirmation(&combined),
        });
    });

    Ok(MonitorHandle {
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

    #[cfg(unix)]
    fn fake_onedrive_binary(
        output: &str,
        exit_code: i32,
    ) -> (std::path::PathBuf, std::path::PathBuf) {
        use std::{
            env,
            os::unix::fs::PermissionsExt,
            time::{SystemTime, UNIX_EPOCH},
        };

        let root = env::temp_dir().join(format!(
            "onesync-fake-onedrive-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let args_file = root.join("args");
        let binary = root.join("fake-onedrive");
        fs::write(
            &binary,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf '%s' '{}'\nexit {}\n",
                args_file.display(),
                output.replace('\'', "'\\''"),
                exit_code
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&binary).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&binary, permissions).unwrap();
        (binary, args_file)
    }

    #[cfg(unix)]
    fn test_account() -> Account {
        use crate::profile::AccountStatus;
        use std::{
            env,
            time::{SystemTime, UNIX_EPOCH},
        };

        let root = env::temp_dir().join(format!(
            "onesync-account-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let config_dir = root.join("profile");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("config"), "sync_dir = \"~/OneDrive\"\n").unwrap();

        Account {
            id: "test-account".to_string(),
            name: "Test Account".to_string(),
            email: String::new(),
            config_dir: config_dir.to_string_lossy().to_string(),
            sync_dir: "~/OneDrive".to_string(),
            status: AccountStatus::Authenticated,
        }
    }

    #[cfg(unix)]
    #[test]
    fn sync_handle_can_stop_running_sync() {
        use crate::profile::AccountStatus;
        use std::{
            env,
            os::unix::fs::PermissionsExt,
            sync::mpsc,
            time::{SystemTime, UNIX_EPOCH},
        };

        let root = env::temp_dir().join(format!(
            "onesync-stop-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let config_dir = root.join("profile");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("config"), "sync_dir = \"~/OneDrive\"\n").unwrap();
        let binary = root.join("fake-onedrive");
        fs::write(
            &binary,
            "#!/bin/sh\ntrap 'exit 0' TERM\nwhile true; do sleep 1; done\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&binary).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&binary, permissions).unwrap();

        let account = Account {
            id: "sync-stop".to_string(),
            name: "Sync Stop".to_string(),
            email: String::new(),
            config_dir: config_dir.to_string_lossy().to_string(),
            sync_dir: "~/OneDrive".to_string(),
            status: AccountStatus::Authenticated,
        };
        let (sender, receiver) = mpsc::channel();
        let handle = start_sync(account, binary.to_string_lossy().to_string(), sender).unwrap();

        stop_handle(&handle).unwrap();
        let event = receiver.recv_timeout(Duration::from_secs(3)).unwrap();
        match event {
            BackendEvent::SyncFinished {
                requested_stop,
                auth_required,
                ..
            } => {
                assert!(requested_stop);
                assert!(!auth_required);
            }
            other => panic!("expected SyncFinished, got {other:?}"),
        }

        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn forced_sync_passes_force_flag_to_onedrive() {
        use crate::profile::AccountStatus;
        use std::{
            env,
            os::unix::fs::PermissionsExt,
            sync::mpsc,
            time::{SystemTime, UNIX_EPOCH},
        };

        let root = env::temp_dir().join(format!(
            "onesync-force-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let config_dir = root.join("profile");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("config"), "sync_dir = \"~/OneDrive\"\n").unwrap();
        let args_file = root.join("args");
        let binary = root.join("fake-onedrive");
        fs::write(
            &binary,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n",
                args_file.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&binary).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&binary, permissions).unwrap();

        let account = Account {
            id: "force-sync".to_string(),
            name: "Force Sync".to_string(),
            email: String::new(),
            config_dir: config_dir.to_string_lossy().to_string(),
            sync_dir: "~/OneDrive".to_string(),
            status: AccountStatus::Authenticated,
        };
        let (sender, receiver) = mpsc::channel();
        let _handle =
            start_forced_sync(account, binary.to_string_lossy().to_string(), sender).unwrap();
        let event = receiver.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(matches!(
            event,
            BackendEvent::SyncFinished {
                success: true,
                requested_stop: false,
                ..
            }
        ));

        let args = fs::read_to_string(args_file).unwrap();
        assert!(args.lines().any(|arg| arg == "--force"));
        assert!(!args.lines().any(|arg| arg == "--resync-auth"));

        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn resync_passes_resync_flag_to_onedrive() {
        use crate::profile::AccountStatus;
        use std::{
            env,
            os::unix::fs::PermissionsExt,
            sync::mpsc,
            time::{SystemTime, UNIX_EPOCH},
        };

        let root = env::temp_dir().join(format!(
            "onesync-resync-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let config_dir = root.join("profile");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("config"), "sync_dir = \"~/OneDrive\"\n").unwrap();
        let args_file = root.join("args");
        let binary = root.join("fake-onedrive");
        fs::write(
            &binary,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n",
                args_file.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&binary).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&binary, permissions).unwrap();

        let account = Account {
            id: "resync".to_string(),
            name: "Resync".to_string(),
            email: String::new(),
            config_dir: config_dir.to_string_lossy().to_string(),
            sync_dir: "~/OneDrive".to_string(),
            status: AccountStatus::Authenticated,
        };
        let (sender, receiver) = mpsc::channel();
        let _handle = start_resync(account, binary.to_string_lossy().to_string(), sender).unwrap();
        let event = receiver.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(matches!(
            event,
            BackendEvent::SyncFinished {
                success: true,
                requested_stop: false,
                ..
            }
        ));

        let args = fs::read_to_string(args_file).unwrap();
        assert!(args.lines().any(|arg| arg == "--resync"));
        assert!(args.lines().any(|arg| arg == "--resync-auth"));

        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn preview_sync_passes_dry_run_flag_to_onedrive() {
        use crate::profile::AccountStatus;
        use std::{
            env,
            os::unix::fs::PermissionsExt,
            sync::mpsc,
            time::{SystemTime, UNIX_EPOCH},
        };

        let root = env::temp_dir().join(format!(
            "onesync-preview-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let config_dir = root.join("profile");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("config"), "sync_dir = \"~/OneDrive\"\n").unwrap();
        let args_file = root.join("args");
        let binary = root.join("fake-onedrive");
        fs::write(
            &binary,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf '%s\\n' 'Uploading new file: ./docs/a.txt ... done'\n",
                args_file.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&binary).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&binary, permissions).unwrap();

        let account = Account {
            id: "preview-sync".to_string(),
            name: "Preview Sync".to_string(),
            email: String::new(),
            config_dir: config_dir.to_string_lossy().to_string(),
            sync_dir: "~/OneDrive".to_string(),
            status: AccountStatus::Authenticated,
        };
        let (sender, receiver) = mpsc::channel();
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
                success: true,
                requested_stop: false,
                ..
            }
        )));

        let args = fs::read_to_string(args_file).unwrap();
        assert!(args.lines().any(|arg| arg == "--sync"));
        assert!(args.lines().any(|arg| arg == "--verbose"));
        assert!(args.lines().any(|arg| arg == "--local-first"));
        assert!(args.lines().any(|arg| arg == "--dry-run"));

        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn preview_handle_reports_requested_stop() {
        use crate::profile::AccountStatus;
        use std::{
            env,
            os::unix::fs::PermissionsExt,
            sync::mpsc,
            time::{SystemTime, UNIX_EPOCH},
        };

        let root = env::temp_dir().join(format!(
            "onesync-preview-stop-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let config_dir = root.join("profile");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("config"), "sync_dir = \"~/OneDrive\"\n").unwrap();
        let binary = root.join("fake-onedrive");
        fs::write(
            &binary,
            "#!/bin/sh\ntrap 'exit 0' TERM\nwhile true; do sleep 1; done\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&binary).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&binary, permissions).unwrap();

        let account = Account {
            id: "preview-stop".to_string(),
            name: "Preview Stop".to_string(),
            email: String::new(),
            config_dir: config_dir.to_string_lossy().to_string(),
            sync_dir: "~/OneDrive".to_string(),
            status: AccountStatus::Authenticated,
        };
        let (sender, receiver) = mpsc::channel();
        let handle = start_preview(account, binary.to_string_lossy().to_string(), sender).unwrap();

        stop_handle(&handle).unwrap();
        let event = receiver.recv_timeout(Duration::from_secs(3)).unwrap();
        match event {
            BackendEvent::PreviewFinished {
                requested_stop,
                auth_required,
                ..
            } => {
                assert!(requested_stop);
                assert!(!auth_required);
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
        let account = test_account();

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
        let account = test_account();

        display_reconcile_status(&account, binary.to_string_lossy().to_string(), "docs/a.txt")
            .expect("status should succeed");

        let args = fs::read_to_string(output_path).expect("args should be captured");
        assert!(args.lines().any(|arg| arg == "--display-sync-status"));
        assert!(args.lines().any(|arg| arg == "--single-directory"));
        assert!(args.lines().any(|arg| arg == "docs"));
    }
}

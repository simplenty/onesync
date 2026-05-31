use super::{
    event::{BackendEvent, ClientCheck, Version},
    output::{
        combined_output, is_auth_required, parse_confirmation, parse_onedrive_error, parse_version,
    },
};
use crate::{
    account::{Account, auth_response_path, auth_url_path, is_authenticated},
    config::ensure_transfer_metrics_enabled,
    transfer::parse_transfer_line,
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
                    None => ClientCheck::Missing("无法解析 onedrive --version 输出".to_string()),
                }
            }
            Err(error) => ClientCheck::Missing(format!("{binary}: {error}")),
        };
        let _ = sender.send(BackendEvent::ClientChecked(result));
    });
}

pub fn start_authentication(account: Account, binary: String, sender: mpsc::Sender<BackendEvent>) {
    thread::spawn(move || {
        let auth_url = auth_url_path(&account);
        let auth_response = auth_response_path(&account);
        let _ = fs::remove_file(&auth_url);
        let _ = fs::remove_file(&auth_response);

        let child = match Command::new(&binary)
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
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                let _ = sender.send(BackendEvent::AuthFinished {
                    account_id: account.id,
                    success: false,
                    message: Some(format!("无法启动认证: {error}")),
                });
                return;
            }
        };

        for _ in 0..300 {
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

        match child.wait_with_output() {
            Ok(output) => {
                let combined = combined_output(&output.stdout, &output.stderr);
                let success = output.status.success() || is_authenticated(&account);
                let message = (!success).then(|| parse_onedrive_error(&combined));
                let _ = sender.send(BackendEvent::AuthFinished {
                    account_id: account.id,
                    success,
                    message,
                });
            }
            Err(error) => {
                let _ = sender.send(BackendEvent::AuthFinished {
                    account_id: account.id,
                    success: false,
                    message: Some(format!("等待认证进程失败: {error}")),
                });
            }
        }
    });
}

pub fn start_sync(
    account: Account,
    binary: String,
    sender: mpsc::Sender<BackendEvent>,
) -> io::Result<SyncHandle> {
    ensure_transfer_metrics_enabled(&account.config_dir)?;

    let mut child = Command::new(binary)
        .arg("--confdir")
        .arg(&account.config_dir)
        .arg("--sync")
        .arg("--verbose")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let output = attach_readers(&account.id, &mut child, &sender);
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
                    message: (!success && !requested_stop).then(|| parse_onedrive_error(&combined)),
                    requires_confirmation: parse_confirmation(&combined),
                });
            }
            Err(error) => {
                let _ = sender.send(BackendEvent::SyncFinished {
                    account_id: account.id,
                    success: false,
                    requested_stop,
                    auth_required: false,
                    message: Some(format!("等待同步进程失败: {error}")),
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

pub fn start_logout(account: Account, binary: String, sender: mpsc::Sender<BackendEvent>) {
    thread::spawn(move || {
        let output = Command::new(&binary)
            .arg("--confdir")
            .arg(&account.config_dir)
            .arg("--logout")
            .output();
        match output {
            Ok(output) => {
                let combined = combined_output(&output.stdout, &output.stderr);
                let _ = sender.send(BackendEvent::LogoutFinished {
                    account_id: account.id,
                    success: output.status.success(),
                    message: (!output.status.success()).then(|| parse_onedrive_error(&combined)),
                });
            }
            Err(error) => {
                let _ = sender.send(BackendEvent::LogoutFinished {
                    account_id: account.id,
                    success: false,
                    message: Some(format!("无法启动 logout: {error}")),
                });
            }
        }
    });
}

pub fn start_monitor(
    account: Account,
    binary: String,
    sender: mpsc::Sender<BackendEvent>,
) -> io::Result<MonitorHandle> {
    ensure_transfer_metrics_enabled(&account.config_dir)?;

    let mut child = Command::new(binary)
        .arg("--confdir")
        .arg(&account.config_dir)
        .arg("--monitor")
        .arg("--verbose")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let output = attach_readers(&account.id, &mut child, &sender);
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
                            message: Some("无法访问持续同步进程".to_string()),
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
                        message: Some(format!("轮询持续同步进程失败: {error}")),
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
            message: (!success).then(|| parse_onedrive_error(&combined)),
            requires_confirmation: parse_confirmation(&combined),
        });
    });

    Ok(MonitorHandle {
        child,
        stop_requested,
    })
}

pub fn stop_handle(handle: &SyncHandle) -> io::Result<()> {
    handle.stop_requested.store(true, Ordering::SeqCst);
    let mut child = handle
        .child
        .lock()
        .map_err(|_| io::Error::other("failed to lock onedrive process"))?;
    terminate_child(&mut child)
}

pub fn stop_monitor_handle(handle: &MonitorHandle) -> io::Result<()> {
    stop_handle(handle)
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
) -> Arc<Mutex<String>> {
    let output = Arc::new(Mutex::new(String::new()));
    if let Some(stdout) = child.stdout.take() {
        spawn_transfer_reader(
            account_id.to_string(),
            stdout,
            sender.clone(),
            Arc::clone(&output),
        );
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_transfer_reader(
            account_id.to_string(),
            stderr,
            sender.clone(),
            Arc::clone(&output),
        );
    }
    output
}

fn spawn_transfer_reader<R>(
    account_id: String,
    reader: R,
    sender: mpsc::Sender<BackendEvent>,
    output: Arc<Mutex<String>>,
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
                        send_transfer_chunk(&account_id, &buffer, &sender, &output);
                        buffer.clear();
                    } else {
                        buffer.push(byte[0]);
                    }
                }
                Err(_) => break,
            }
        }
        if !buffer.is_empty() {
            send_transfer_chunk(&account_id, &buffer, &sender, &output);
        }
    });
}

fn send_transfer_chunk(
    account_id: &str,
    chunk: &[u8],
    sender: &mpsc::Sender<BackendEvent>,
    output: &Arc<Mutex<String>>,
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
    if let Some(file) = parse_transfer_line(line) {
        let _ = sender.send(BackendEvent::TransferEvent {
            account_id: account_id.to_string(),
            file,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn sync_handle_can_stop_running_sync() {
        use crate::account::AccountStatus;
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
}

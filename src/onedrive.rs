use crate::{
    account::{Account, auth_response_path, auth_url_path, is_authenticated},
    config::ensure_transfer_metrics_enabled,
    transfer::{SyncFile, parse_transfer_line},
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

pub const MIN_ONEDRIVE_VERSION: Version = Version {
    major: 2,
    minor: 5,
    patch: 0,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

#[derive(Debug, Clone)]
pub enum ClientCheck {
    Unknown,
    Ready(Version),
    Missing(String),
    Unsupported { found: Version, minimum: Version },
}

impl ClientCheck {
    #[must_use]
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready(_))
    }

    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::Unknown => "正在检测 onedrive CLI".to_string(),
            Self::Ready(version) => format!(
                "onedrive CLI {}.{}.{} 可用",
                version.major, version.minor, version.patch
            ),
            Self::Missing(error) => format!("未找到 onedrive CLI: {error}"),
            Self::Unsupported { found, minimum } => format!(
                "onedrive CLI 版本过低: 当前 {}.{}.{}, 需要 >= {}.{}.{}",
                found.major, found.minor, found.patch, minimum.major, minimum.minor, minimum.patch
            ),
        }
    }
}

#[derive(Debug)]
pub enum BackendEvent {
    ClientChecked(ClientCheck),
    AuthUrl {
        account_id: String,
        url: String,
    },
    AuthFinished {
        account_id: String,
        success: bool,
        message: Option<String>,
    },
    SyncFinished {
        account_id: String,
        success: bool,
        requested_stop: bool,
        auth_required: bool,
        message: Option<String>,
        requires_confirmation: Option<ConfirmationKind>,
    },
    LogoutFinished {
        account_id: String,
        success: bool,
        message: Option<String>,
    },
    TransferEvent {
        account_id: String,
        file: SyncFile,
    },
    MonitorStopped {
        account_id: String,
        success: bool,
        requested_stop: bool,
        auth_required: bool,
        message: Option<String>,
        requires_confirmation: Option<ConfirmationKind>,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum ConfirmationKind {
    ResyncRequired,
    BigDelete,
    DownloadOnlyCleanup,
    UploadOnlyNoRemoteDelete,
}

impl ConfirmationKind {
    #[must_use]
    pub fn user_message(self) -> &'static str {
        match self {
            Self::ResyncRequired => {
                "onedrive 要求执行 --resync。请确认该 profile 的本地与远端状态后再手动处理。"
            }
            Self::BigDelete => "onedrive 检测到大量删除，需要授权。请先检查删除列表后再继续。",
            Self::DownloadOnlyCleanup => "download-only 清理可能删除本地文件。请确认配置后再继续。",
            Self::UploadOnlyNoRemoteDelete => {
                "upload-only 与 no-remote-delete 组合需要显式确认兼容性。"
            }
        }
    }
}

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

fn parse_version(output: &str) -> Option<Version> {
    let (_, version) = output.split_once("onedrive")?;
    let mut parts = version
        .split(|character: char| !character.is_ascii_digit() && character != '.')
        .find(|part| part.chars().any(|character| character.is_ascii_digit()))?
        .split('.');
    Some(Version {
        major: parts.next()?.parse().ok()?,
        minor: parts.next().unwrap_or("0").parse().ok()?,
        patch: parts.next().unwrap_or("0").parse().ok()?,
    })
}

fn combined_output(stdout: &[u8], stderr: &[u8]) -> String {
    let mut combined = String::from_utf8_lossy(stdout).to_string();
    combined.push_str(&String::from_utf8_lossy(stderr));
    combined
}

fn parse_onedrive_error(output: &str) -> String {
    let lower = output.to_ascii_lowercase();
    if is_auth_required(output) {
        "认证已过期或缺少 refresh_token，请重新完成登录".to_string()
    } else if lower.contains("could not resolve")
        || lower.contains("connection")
        || lower.contains("network")
        || lower.contains("timeout")
    {
        "网络连接失败，请检查网络或代理后重试".to_string()
    } else if lower.contains("unknown key") || lower.contains("unknown config") {
        "配置文件包含 onedrive 不支持的选项，请编辑 profile 配置".to_string()
    } else if lower.contains("failed") && (lower.contains("upload") || lower.contains("download")) {
        "部分上传或下载失败，请查看传输列表和 onedrive 输出".to_string()
    } else if lower.contains("segmentation fault") || lower.contains("core dumped") {
        "onedrive CLI 崩溃，请升级 CLI 或检查该 profile 配置".to_string()
    } else if lower.contains("auth") || lower.contains("unauthorized") {
        "认证失败，请重新完成该 profile 登录".to_string()
    } else {
        output
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("onedrive 操作失败")
            .trim()
            .to_string()
    }
}

fn is_auth_required(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("login required")
        || lower.contains("authorise this application")
        || lower.contains("authorize this application")
        || lower.contains("refresh_token is invalid")
        || lower.contains("refresh token is invalid")
        || lower.contains("reauth")
}

fn parse_confirmation(output: &str) -> Option<ConfirmationKind> {
    let lower = output.to_ascii_lowercase();
    if lower.contains("--resync") && lower.contains("required") {
        Some(ConfirmationKind::ResyncRequired)
    } else if lower.contains("big delete") || lower.contains("large delete") {
        Some(ConfirmationKind::BigDelete)
    } else if lower.contains("download-only") && lower.contains("cleanup") {
        Some(ConfirmationKind::DownloadOnlyCleanup)
    } else if lower.contains("upload-only") && lower.contains("no-remote-delete") {
        Some(ConfirmationKind::UploadOnlyNoRemoteDelete)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_version_from_cli_output() {
        assert_eq!(
            parse_version("onedrive v2.5.4-1+np1").unwrap(),
            Version {
                major: 2,
                minor: 5,
                patch: 4
            }
        );
    }

    #[test]
    fn maps_known_error_output_to_actionable_messages() {
        assert_eq!(
            parse_onedrive_error("ERROR: refresh_token is invalid"),
            "认证已过期或缺少 refresh_token，请重新完成登录"
        );
        assert_eq!(
            parse_onedrive_error("curl timeout while connecting"),
            "网络连接失败，请检查网络或代理后重试"
        );
        assert_eq!(
            parse_onedrive_error("unknown config key: verbose"),
            "配置文件包含 onedrive 不支持的选项，请编辑 profile 配置"
        );
    }

    #[test]
    fn detects_login_required_output() {
        assert!(is_auth_required("ERROR: Login required"));
        assert!(is_auth_required(
            "To authorise this application open the URL"
        ));
        assert!(is_auth_required("ERROR: refresh_token is invalid"));
    }

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

    #[test]
    fn detects_confirmation_required_states() {
        assert!(matches!(
            parse_confirmation("--resync is required to continue"),
            Some(ConfirmationKind::ResyncRequired)
        ));
        assert!(matches!(
            parse_confirmation("ERROR: big delete detected"),
            Some(ConfirmationKind::BigDelete)
        ));
        assert!(matches!(
            parse_confirmation("download-only cleanup warning"),
            Some(ConfirmationKind::DownloadOnlyCleanup)
        ));
        assert!(matches!(
            parse_confirmation("upload-only cannot be used with no-remote-delete"),
            Some(ConfirmationKind::UploadOnlyNoRemoteDelete)
        ));
    }
}

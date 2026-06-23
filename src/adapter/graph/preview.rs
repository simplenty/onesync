use super::http::response_to_io;
use super::identity::graph_access_token;
use crate::event::{BackendError, BackendEvent};
use crate::{
    event::payload::{PreviewAction, PreviewChange},
    profile::Account,
    utils::expand_home,
};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::{
    fs,
    io::Write,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};

const GRAPH_ROOT: &str = "https://graph.microsoft.com/v1.0";
const SIMPLE_UPLOAD_LIMIT: u64 = 250 * 1024 * 1024;
const LARGE_UPLOAD_CHUNK: u64 = 10 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct DriveItemResponse {
    id: String,
    #[serde(default)]
    size: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadSessionResponse {
    upload_url: String,
}

pub fn start_apply_preview_change(
    account: Account,
    change: PreviewChange,
    binary: String,
    sender: mpsc::Sender<BackendEvent>,
) {
    thread::spawn(move || {
        let change_id = change.id.clone();
        let result = apply_preview_change(&account, &change, &sender)
            .and_then(|_| finish_graph_apply_with_reconcile(&account, &change, binary, &sender));
        let _ = sender.send(BackendEvent::PreviewApplyFinished {
            account_id: account.id,
            change_id,
            success: result.is_ok(),
            error: result
                .err()
                .map(|e| BackendError::ApplyFailed(e.to_string())),
        });
    });
}

fn apply_preview_change(
    account: &Account,
    change: &PreviewChange,
    sender: &mpsc::Sender<BackendEvent>,
) -> io::Result<()> {
    let access_token = graph_access_token(account)?;
    let client = Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(io::Error::other)?;

    match change.apply {
        PreviewAction::UploadLocalToRemote => {
            upload_local_file(&client, &access_token, account, change, sender)
        }
        PreviewAction::DownloadRemoteToLocal => {
            download_remote_file(&client, &access_token, account, change, sender)
        }
        PreviewAction::DeleteRemote => delete_remote_item(&client, &access_token, &change.path),
        PreviewAction::DeleteLocal => delete_local_item(account, &change.path),
        PreviewAction::MoveRemoteItem | PreviewAction::RenameRemoteItem => {
            move_remote_item(&client, &access_token, change)
        }
    }
}

fn finish_graph_apply_with_reconcile(
    account: &Account,
    change: &PreviewChange,
    binary: String,
    sender: &mpsc::Sender<BackendEvent>,
) -> io::Result<()> {
    let _ = sender.send(BackendEvent::PreviewReconcileStarted {
        account_id: account.id.clone(),
        change_id: change.id.clone(),
    });

    let reconcile =
        crate::adapter::onedrive::reconcile_preview_change(account, binary.clone(), &change.path)
            .and_then(|_| {
                crate::adapter::onedrive::display_reconcile_status(account, binary, &change.path)
            });

    let success = reconcile.is_ok();
    let error = reconcile
        .as_ref()
        .err()
        .map(|e| BackendError::ReconcileFailed(e.to_string()));
    let _ = sender.send(BackendEvent::PreviewReconcileFinished {
        account_id: account.id.clone(),
        change_id: change.id.clone(),
        success,
        error: error.clone(),
    });

    reconcile.map(|_| ())
}

fn upload_local_file(
    client: &Client,
    token: &str,
    account: &Account,
    change: &PreviewChange,
    sender: &mpsc::Sender<BackendEvent>,
) -> io::Result<()> {
    let path = change.path.as_str();
    let local_path = local_path(account, path);
    let size = fs::metadata(&local_path)?.len();
    ensure_remote_parent_dirs(client, token, path)?;
    send_apply_progress(sender, &account.id, &change.id, 0.0);
    if size <= SIMPLE_UPLOAD_LIMIT {
        let file = fs::File::open(&local_path)?;
        let body = ProgressReader::new(
            file,
            size,
            account.id.clone(),
            change.id.clone(),
            sender.clone(),
        );
        let url = format!("{GRAPH_ROOT}/me/drive/root:/{}:/content", graph_path(path));
        response_to_io(
            client
                .put(url)
                .bearer_auth(token)
                .header("Content-Length", size.to_string())
                .body(reqwest::blocking::Body::new(body))
                .send(),
        )?;
        send_apply_progress(sender, &account.id, &change.id, 1.0);
        Ok(())
    } else {
        upload_large_file(client, token, account, change, &local_path, size, sender)
    }
}

fn upload_large_file(
    client: &Client,
    token: &str,
    account: &Account,
    change: &PreviewChange,
    local_path: &Path,
    size: u64,
    sender: &mpsc::Sender<BackendEvent>,
) -> io::Result<()> {
    let url = format!(
        "{GRAPH_ROOT}/me/drive/root:/{}:/createUploadSession",
        graph_path(&change.path)
    );
    let session = response_to_io(
        client
            .post(url)
            .bearer_auth(token)
            .json(
                &serde_json::json!({ "item": { "@microsoft.graph.conflictBehavior": "replace" } }),
            )
            .send(),
    )?
    .json::<UploadSessionResponse>()
    .map_err(io::Error::other)?;

    let mut file = fs::File::open(local_path)?;
    let mut offset = 0_u64;
    while offset < size {
        let next = (offset + LARGE_UPLOAD_CHUNK).min(size);
        let length = next - offset;
        let mut buffer = vec![0_u8; length as usize];
        file.read_exact(&mut buffer)?;
        response_to_io(
            client
                .put(&session.upload_url)
                .header("Content-Length", length.to_string())
                .header(
                    "Content-Range",
                    format!("bytes {}-{}/{}", offset, next - 1, size),
                )
                .body(buffer)
                .send(),
        )?;
        offset = next;
        send_apply_progress(
            sender,
            &account.id,
            &change.id,
            offset as f64 / size.max(1) as f64,
        );
    }

    Ok(())
}

struct ProgressReader<R> {
    inner: R,
    total: u64,
    read: u64,
    account_id: String,
    change_id: String,
    sender: mpsc::Sender<BackendEvent>,
}

impl<R> ProgressReader<R> {
    fn new(
        inner: R,
        total: u64,
        account_id: String,
        change_id: String,
        sender: mpsc::Sender<BackendEvent>,
    ) -> Self {
        Self {
            inner,
            total,
            read: 0,
            account_id,
            change_id,
            sender,
        }
    }
}

impl<R: Read> Read for ProgressReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let bytes_read = self.inner.read(buffer)?;
        if bytes_read > 0 {
            self.read = self.read.saturating_add(bytes_read as u64);
            send_apply_progress(
                &self.sender,
                &self.account_id,
                &self.change_id,
                self.read as f64 / self.total.max(1) as f64,
            );
        }
        Ok(bytes_read)
    }
}

fn send_apply_progress(
    sender: &mpsc::Sender<BackendEvent>,
    account_id: &str,
    change_id: &str,
    progress: f64,
) {
    let _ = sender.send(BackendEvent::PreviewApplyProgress {
        account_id: account_id.to_string(),
        change_id: change_id.to_string(),
        progress: progress.clamp(0.0, 1.0),
    });
}

fn download_remote_file(
    client: &Client,
    token: &str,
    account: &Account,
    change: &PreviewChange,
    sender: &mpsc::Sender<BackendEvent>,
) -> io::Result<()> {
    let path = change.path.as_str();
    let item = get_drive_item(client, token, path)?;
    let url = format!("{GRAPH_ROOT}/me/drive/root:/{}:/content", graph_path(path));
    let mut response = response_to_io(client.get(url).bearer_auth(token).send())?;
    let total = if item.size > 0 {
        item.size
    } else {
        response.content_length().unwrap_or(0)
    };
    let local_path = local_path(account, path);
    if let Some(parent) = local_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = local_path.with_extension("onesync-download");
    let mut temp_file = fs::File::create(&temp_path)?;
    let mut downloaded = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    send_apply_progress(sender, &account.id, &change.id, 0.0);
    loop {
        let bytes_read = response.read(&mut buffer).map_err(io::Error::other)?;
        if bytes_read == 0 {
            break;
        }
        temp_file.write_all(&buffer[..bytes_read])?;
        downloaded = downloaded.saturating_add(bytes_read as u64);
        if total > 0 {
            send_apply_progress(
                sender,
                &account.id,
                &change.id,
                downloaded as f64 / total as f64,
            );
        }
    }
    temp_file.flush()?;
    fs::rename(temp_path, local_path)?;
    send_apply_progress(sender, &account.id, &change.id, 1.0);
    Ok(())
}

fn delete_remote_item(client: &Client, token: &str, path: &str) -> io::Result<()> {
    let item = get_drive_item(client, token, path)?;
    response_to_io(
        client
            .delete(format!("{GRAPH_ROOT}/me/drive/items/{}", item.id))
            .bearer_auth(token)
            .send(),
    )?;
    Ok(())
}

fn delete_local_item(account: &Account, path: &str) -> io::Result<()> {
    let local_path = local_path(account, path);
    if local_path.is_dir() {
        fs::remove_dir(&local_path)
    } else {
        fs::remove_file(&local_path)
    }
}

fn move_remote_item(client: &Client, token: &str, change: &PreviewChange) -> io::Result<()> {
    let source = change.source_path.as_deref().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "move or rename preview is missing source path",
        )
    })?;
    let source_item = get_drive_item(client, token, source)?;
    let target = crate::utils::sync_path(&change.path);
    let parent_item = match target.parent {
        Some(parent) => get_drive_item(client, token, parent)?,
        None => get_drive_item(client, token, "")?,
    };

    response_to_io(
        client
            .patch(format!("{GRAPH_ROOT}/me/drive/items/{}", source_item.id))
            .bearer_auth(token)
            .json(&serde_json::json!({
                "parentReference": { "id": parent_item.id },
                "name": target.name,
            }))
            .send(),
    )?;
    Ok(())
}

fn ensure_remote_parent_dirs(client: &Client, token: &str, path: &str) -> io::Result<()> {
    let Some(parent) = crate::utils::sync_path(path).parent else {
        return Ok(());
    };

    let mut current = String::new();
    for segment in parent.split('/').filter(|segment| !segment.is_empty()) {
        current = if current.is_empty() {
            segment.to_string()
        } else {
            format!("{current}/{segment}")
        };
        if get_drive_item(client, token, &current).is_err() {
            create_remote_folder(client, token, &current)?;
        }
    }

    Ok(())
}

fn create_remote_folder(client: &Client, token: &str, path: &str) -> io::Result<()> {
    let folder = crate::utils::sync_path(path);
    let url = match folder.parent {
        Some(parent) => {
            let parent_item = get_drive_item(client, token, parent)?;
            format!("{GRAPH_ROOT}/me/drive/items/{}/children", parent_item.id)
        }
        None => format!("{GRAPH_ROOT}/me/drive/root/children"),
    };

    response_to_io(
        client
            .post(url)
            .bearer_auth(token)
            .json(&serde_json::json!({
                "name": folder.name,
                "folder": {},
                "@microsoft.graph.conflictBehavior": "fail",
            }))
            .send(),
    )?;
    Ok(())
}

fn get_drive_item(client: &Client, token: &str, path: &str) -> io::Result<DriveItemResponse> {
    let url = if path.is_empty() {
        format!("{GRAPH_ROOT}/me/drive/root")
    } else {
        format!("{GRAPH_ROOT}/me/drive/root:/{}", graph_path(path))
    };
    response_to_io(client.get(url).bearer_auth(token).send())?
        .json::<DriveItemResponse>()
        .map_err(io::Error::other)
}

fn local_path(account: &Account, path: &str) -> PathBuf {
    expand_home(&account.sync_dir).join(path.trim_start_matches("./"))
}

fn graph_path(path: &str) -> String {
    path.trim_start_matches("./")
        .split('/')
        .map(percent_encode_segment)
        .collect::<Vec<_>>()
        .join("/")
}

fn percent_encode_segment(segment: &str) -> String {
    let mut encoded = String::new();
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::test_support::{fake_onedrive_binary, temp_account};
    use crate::event::payload::{ChangeKind, PreviewIntent, PreviewState};
    use std::{io::Cursor, sync::mpsc};

    fn preview_change(path: &str, apply: PreviewAction) -> PreviewChange {
        PreviewChange {
            id: format!("upload-new:{path}"),
            path: path.to_string(),
            source_path: None,
            kind: ChangeKind::UploadNew,
            apply,
            intent: PreviewIntent::LocalChangeToRemote,
            state: PreviewState::Pending,
        }
    }

    #[test]
    fn graph_path_encodes_spaces_and_hashes_but_keeps_slashes() {
        assert_eq!(graph_path("Folder A/a#b.txt"), "Folder%20A/a%23b.txt");
    }

    #[test]
    fn drive_item_size_defaults_when_missing() {
        let item: DriveItemResponse = serde_json::from_str(r#"{"id":"abc"}"#).unwrap();
        assert_eq!(item.id, "abc");
        assert_eq!(item.size, 0);

        let item: DriveItemResponse = serde_json::from_str(r#"{"id":"abc","size":4096}"#).unwrap();
        assert_eq!(item.size, 4096);
    }

    #[test]
    fn progress_reader_reports_apply_progress() {
        let (sender, receiver) = mpsc::channel();
        let mut reader = ProgressReader::new(
            Cursor::new(vec![1_u8, 2, 3, 4]),
            4,
            "account".to_string(),
            "change".to_string(),
            sender,
        );
        let mut buffer = Vec::new();

        reader.read_to_end(&mut buffer).unwrap();

        assert_eq!(buffer, vec![1, 2, 3, 4]);
        let events: Vec<BackendEvent> = receiver.try_iter().collect();
        assert!(events.iter().any(|event| matches!(
            event,
            BackendEvent::PreviewApplyProgress {
                account_id,
                change_id,
                progress,
            } if account_id == "account" && change_id == "change" && (*progress - 1.0).abs() < f64::EPSILON
        )));
    }

    #[cfg(unix)]
    #[test]
    fn successful_graph_apply_emits_reconcile_events() {
        let (sender, receiver) = mpsc::channel();
        let account = temp_account("graph");
        let (binary, _output_path) = fake_onedrive_binary(
            "Sync with Microsoft OneDrive is complete\nThe directory is in sync\n",
            0,
        );
        let change = preview_change("docs/a.txt", PreviewAction::UploadLocalToRemote);

        finish_graph_apply_with_reconcile(
            &account,
            &change,
            binary.to_string_lossy().to_string(),
            &sender,
        )
        .expect("reconcile should succeed");

        let events: Vec<BackendEvent> = receiver.try_iter().collect();
        assert!(events.iter().any(|event| matches!(
            event,
            BackendEvent::PreviewReconcileStarted { change_id, .. }
                if change_id == "upload-new:docs/a.txt"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            BackendEvent::PreviewReconcileFinished { change_id, success: true, .. }
                if change_id == "upload-new:docs/a.txt"
        )));
    }
}

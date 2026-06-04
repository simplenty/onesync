#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransferKind {
    Download,
    UploadNew,
    UploadModified,
    DeleteLocal,
    DeleteRemote,
    Move,
    Rename,
}

impl TransferKind {
    #[must_use]
    pub fn action_label(self) -> &'static str {
        match self {
            Self::Download => "下载",
            Self::UploadNew => "上传",
            Self::UploadModified => "更新",
            Self::DeleteLocal | Self::DeleteRemote => "删除",
            Self::Move => "移动",
            Self::Rename => "重命名",
        }
    }

    #[must_use]
    pub fn icon_name(self) -> &'static str {
        match self {
            Self::Download => DOWNLOAD_ICON,
            Self::UploadNew => UPLOAD_ICON,
            Self::UploadModified => UPDATE_ICON,
            Self::DeleteLocal | Self::DeleteRemote => DELETE_ICON,
            Self::Move => MOVE_ICON,
            Self::Rename => RENAME_ICON,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransferDirection {
    LocalToRemote,
    RemoteToLocal,
    RemoteMetadata,
}

#[derive(Debug, Clone)]
pub struct SyncFile {
    pub name: String,
    pub state: String,
    pub progress: f64,
    pub icon: &'static str,
    pub kind: TransferKind,
    pub direction: TransferDirection,
}

impl SyncFile {
    pub fn is_complete(&self) -> bool {
        self.progress >= 1.0 || self.state.ends_with("完成")
    }

    pub fn is_failed(&self) -> bool {
        self.state.ends_with("失败")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum PreviewState {
    Pending,
    Applying,
    Reconciling,
    Applied,
    Failed,
    ReconcileFailed,
    Dismissed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreviewApply {
    UploadLocalToRemote,
    DownloadRemoteToLocal,
    DeleteRemote,
    DeleteLocal,
    MoveRemoteItem,
    RenameRemoteItem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum PreviewIntent {
    LocalChangeToRemote,
    RemoteChangeToLocal,
    LocalDeleteToRemote,
    RemoteDeleteToLocal,
    RemoteMetadataChange,
    AmbiguousRemoteToLocal,
}

impl PreviewIntent {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::LocalChangeToRemote => "本地新增或修改",
            Self::RemoteChangeToLocal => "远端新增或更新",
            Self::LocalDeleteToRemote => "本地删除",
            Self::RemoteDeleteToLocal => "远端删除",
            Self::RemoteMetadataChange => "远端元数据变更",
            Self::AmbiguousRemoteToLocal => "远端存在/本地缺失语义不唯一",
        }
    }

    #[must_use]
    pub fn detail(self) -> &'static str {
        match self {
            Self::LocalChangeToRemote => "本地内容会同步到 OneDrive，完成后会更新同步状态。",
            Self::RemoteChangeToLocal => "OneDrive 内容会下载到本地，完成后会更新同步状态。",
            Self::LocalDeleteToRemote => {
                "本地已删除的项目也会从 OneDrive 删除，完成后会更新同步状态。"
            }
            Self::RemoteDeleteToLocal => {
                "OneDrive 已删除的项目也会从本地删除，完成后会更新同步状态。"
            }
            Self::RemoteMetadataChange => {
                "OneDrive 上的名称或位置变化会同步到本地，完成后会更新同步状态。"
            }
            Self::AmbiguousRemoteToLocal => {
                "这个项目需要你确认后再应用，避免把不确定的变化自动同步。"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreviewBasis {
    CliDryRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum PreviewConfidence {
    Exact,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewChange {
    pub id: String,
    pub path: String,
    pub source_path: Option<String>,
    pub kind: TransferKind,
    pub direction: TransferDirection,
    pub apply: PreviewApply,
    pub intent: PreviewIntent,
    pub basis: PreviewBasis,
    pub confidence: PreviewConfidence,
    pub state: PreviewState,
    pub description: String,
    pub icon: &'static str,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum TransferItem {
    Live(SyncFile),
    Preview(PreviewChange),
}

#[derive(Clone, Copy)]
struct TransferPattern {
    prefix: &'static str,
    kind: TransferKind,
    direction: TransferDirection,
    complete_without_done: bool,
}

const fn pattern(
    prefix: &'static str,
    kind: TransferKind,
    direction: TransferDirection,
    complete_without_done: bool,
) -> TransferPattern {
    TransferPattern {
        prefix,
        kind,
        direction,
        complete_without_done,
    }
}

const DOWNLOAD_ICON: &str = "go-down-symbolic";
const UPLOAD_ICON: &str = "go-up-symbolic";
const UPDATE_ICON: &str = "document-save-symbolic";
const DELETE_ICON: &str = "edit-delete-symbolic";
const MOVE_ICON: &str = "go-jump-symbolic";
const RENAME_ICON: &str = "document-edit-symbolic";

const TRANSFER_PATTERNS: &[TransferPattern] = &[
    pattern(
        "Downloading file:",
        TransferKind::Download,
        TransferDirection::RemoteToLocal,
        false,
    ),
    pattern(
        "Downloading file",
        TransferKind::Download,
        TransferDirection::RemoteToLocal,
        false,
    ),
    pattern(
        "Downloading:",
        TransferKind::Download,
        TransferDirection::RemoteToLocal,
        false,
    ),
    pattern(
        "Uploading:",
        TransferKind::UploadNew,
        TransferDirection::LocalToRemote,
        false,
    ),
    pattern(
        "Uploading modified file:",
        TransferKind::UploadModified,
        TransferDirection::LocalToRemote,
        false,
    ),
    pattern(
        "Uploading modified file",
        TransferKind::UploadModified,
        TransferDirection::LocalToRemote,
        false,
    ),
    pattern(
        "Uploading new file:",
        TransferKind::UploadNew,
        TransferDirection::LocalToRemote,
        false,
    ),
    pattern(
        "Uploading new file",
        TransferKind::UploadNew,
        TransferDirection::LocalToRemote,
        false,
    ),
    pattern(
        "Uploading file:",
        TransferKind::UploadNew,
        TransferDirection::LocalToRemote,
        false,
    ),
    pattern(
        "Uploading file",
        TransferKind::UploadNew,
        TransferDirection::LocalToRemote,
        false,
    ),
    pattern(
        "Deleting item from Microsoft OneDrive:",
        TransferKind::DeleteRemote,
        TransferDirection::LocalToRemote,
        true,
    ),
    pattern(
        "Deleting local file:",
        TransferKind::DeleteLocal,
        TransferDirection::RemoteToLocal,
        true,
    ),
    pattern(
        "Deleting remote file:",
        TransferKind::DeleteRemote,
        TransferDirection::LocalToRemote,
        true,
    ),
    pattern(
        "Deleting file:",
        TransferKind::DeleteRemote,
        TransferDirection::LocalToRemote,
        true,
    ),
    pattern(
        "Deleting local item:",
        TransferKind::DeleteLocal,
        TransferDirection::RemoteToLocal,
        true,
    ),
    pattern(
        "Deleting remote item:",
        TransferKind::DeleteRemote,
        TransferDirection::LocalToRemote,
        true,
    ),
    pattern(
        "Deleting item:",
        TransferKind::DeleteRemote,
        TransferDirection::LocalToRemote,
        true,
    ),
    pattern(
        "Moving file:",
        TransferKind::Move,
        TransferDirection::RemoteMetadata,
        false,
    ),
    pattern(
        "Renaming file:",
        TransferKind::Rename,
        TransferDirection::RemoteMetadata,
        false,
    ),
];

pub fn parse_transfer_line(line: &str) -> Option<SyncFile> {
    let trimmed = line.trim();
    let (pattern, mut path) = TRANSFER_PATTERNS
        .iter()
        .find_map(|pattern| match_transfer_prefix(trimmed, pattern))?;

    let failed = path.ends_with("failed!") || path.contains(" ... failed");
    let done = !failed
        && (pattern.complete_without_done
            || path.ends_with("done.")
            || path.ends_with("done")
            || path.contains(" ... done"));

    if let Some((file_path, _)) = path.split_once(" ... ") {
        path = file_path.trim();
    }
    let progress = parse_percent(trimmed).unwrap_or(if failed {
        0.0
    } else if done {
        1.0
    } else {
        0.0
    });

    let action = pattern.kind.action_label();
    let state = if failed {
        format!("{action}失败")
    } else if done || progress >= 1.0 {
        format!("{action}完成")
    } else {
        format!("正在{action}")
    };

    Some(SyncFile {
        name: path.to_string(),
        state,
        progress,
        icon: pattern.kind.icon_name(),
        kind: pattern.kind,
        direction: pattern.direction,
    })
}

pub fn parse_preview_line(line: &str) -> Option<PreviewChange> {
    let file = parse_transfer_line(line)?;
    let (source_path, path) = split_source_target(&file.name)
        .map_or((None, file.name.clone()), |(source, target)| {
            (Some(source.to_string()), target.to_string())
        });
    let apply = preview_apply_for(file.kind, file.direction);
    let intent = preview_intent_for(file.kind, file.direction, apply);
    let confidence = preview_confidence_for(intent);
    let id = preview_id(file.kind, &path, source_path.as_deref());
    let description = preview_description(apply).to_string();

    Some(PreviewChange {
        id,
        path,
        source_path,
        kind: file.kind,
        direction: file.direction,
        apply,
        intent,
        basis: PreviewBasis::CliDryRun,
        confidence,
        state: PreviewState::Pending,
        description,
        icon: file.icon,
    })
}

fn preview_apply_for(kind: TransferKind, direction: TransferDirection) -> PreviewApply {
    match (kind, direction) {
        (TransferKind::Download, _) => PreviewApply::DownloadRemoteToLocal,
        (TransferKind::UploadNew | TransferKind::UploadModified, _) => {
            PreviewApply::UploadLocalToRemote
        }
        (TransferKind::DeleteLocal, _) => PreviewApply::DeleteLocal,
        (TransferKind::DeleteRemote, _) => PreviewApply::DeleteRemote,
        (TransferKind::Move, _) => PreviewApply::MoveRemoteItem,
        (TransferKind::Rename, _) => PreviewApply::RenameRemoteItem,
    }
}

fn preview_intent_for(
    kind: TransferKind,
    direction: TransferDirection,
    apply: PreviewApply,
) -> PreviewIntent {
    match (kind, direction, apply) {
        (
            TransferKind::Download,
            TransferDirection::RemoteToLocal,
            PreviewApply::DownloadRemoteToLocal,
        ) => PreviewIntent::RemoteChangeToLocal,
        (
            TransferKind::UploadNew | TransferKind::UploadModified,
            TransferDirection::LocalToRemote,
            PreviewApply::UploadLocalToRemote,
        ) => PreviewIntent::LocalChangeToRemote,
        (TransferKind::DeleteRemote, _, PreviewApply::DeleteRemote) => {
            PreviewIntent::LocalDeleteToRemote
        }
        (TransferKind::DeleteLocal, _, PreviewApply::DeleteLocal) => {
            PreviewIntent::RemoteDeleteToLocal
        }
        (TransferKind::Move | TransferKind::Rename, _, _) => PreviewIntent::RemoteMetadataChange,
        _ => PreviewIntent::RemoteChangeToLocal,
    }
}

fn preview_confidence_for(intent: PreviewIntent) -> PreviewConfidence {
    match intent {
        PreviewIntent::AmbiguousRemoteToLocal => PreviewConfidence::Ambiguous,
        PreviewIntent::LocalChangeToRemote
        | PreviewIntent::RemoteChangeToLocal
        | PreviewIntent::LocalDeleteToRemote
        | PreviewIntent::RemoteDeleteToLocal
        | PreviewIntent::RemoteMetadataChange => PreviewConfidence::Exact,
    }
}

fn preview_description(apply: PreviewApply) -> &'static str {
    match apply {
        PreviewApply::UploadLocalToRemote => "将上传到 OneDrive",
        PreviewApply::DownloadRemoteToLocal => "将下载到本地",
        PreviewApply::DeleteRemote => "将从 OneDrive 删除",
        PreviewApply::DeleteLocal => "将从本地删除",
        PreviewApply::MoveRemoteItem => "将在 OneDrive 移动",
        PreviewApply::RenameRemoteItem => "将在 OneDrive 重命名",
    }
}

fn preview_id(kind: TransferKind, path: &str, source_path: Option<&str>) -> String {
    let prefix = match kind {
        TransferKind::Download => "download",
        TransferKind::UploadNew => "upload-new",
        TransferKind::UploadModified => "upload-modified",
        TransferKind::DeleteLocal => "delete-local",
        TransferKind::DeleteRemote => "delete-remote",
        TransferKind::Move => "move",
        TransferKind::Rename => "rename",
    };
    match source_path {
        Some(source) => format!("{prefix}:{source}->{path}"),
        None => format!("{prefix}:{path}"),
    }
}

fn split_source_target(path: &str) -> Option<(&str, &str)> {
    path.split_once(" -> ")
        .or_else(|| path.split_once(" to "))
        .map(|(source, target)| (source.trim(), target.trim()))
        .filter(|(source, target)| !source.is_empty() && !target.is_empty())
}

fn match_transfer_prefix<'a>(
    line: &'a str,
    pattern: &'a TransferPattern,
) -> Option<(&'a TransferPattern, &'a str)> {
    let rest = line.strip_prefix(pattern.prefix)?;
    if rest.is_empty() || rest.starts_with(':') || rest.starts_with(char::is_whitespace) {
        Some((pattern, rest.trim_start_matches(':').trim()))
    } else {
        None
    }
}

fn parse_percent(line: &str) -> Option<f64> {
    let percent_index = line.find('%')?;
    let before_percent = &line[..percent_index];
    let digits_start = before_percent
        .char_indices()
        .rev()
        .find(|(_, character)| !character.is_ascii_digit() && *character != '.')?
        .0
        + 1;
    let percent = before_percent[digits_start..].trim().parse::<f64>().ok()?;
    Some((percent / 100.0).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(line: &str) -> SyncFile {
        parse_transfer_line(line).expect("line should be parsed as a transfer event")
    }

    fn assert_progress(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < f64::EPSILON);
    }

    #[test]
    fn parses_actual_upload_new_file_output() {
        let file = parsed("Uploading new file: ./.onesync-parser-test/move-source.txt ... done");

        assert_eq!(file.name, "./.onesync-parser-test/move-source.txt");
        assert_eq!(file.state, "上传完成");
        assert_eq!(file.icon, "go-up-symbolic");
        assert_progress(file.progress, 1.0);
    }

    #[test]
    fn parses_actual_upload_progress_output() {
        let file = parsed(
            "Uploading: ./.onesync-progress-test/upload-progress.bin ... 37%  |  ETA    00:00:10",
        );

        assert_eq!(file.name, "./.onesync-progress-test/upload-progress.bin");
        assert_eq!(file.state, "正在上传");
        assert_eq!(file.icon, "go-up-symbolic");
        assert_progress(file.progress, 0.37);
    }

    #[test]
    fn parses_actual_delete_item_output() {
        let file =
            parsed("Deleting item from Microsoft OneDrive: .onesync-parser-test/move-source.txt");

        assert_eq!(file.name, ".onesync-parser-test/move-source.txt");
        assert_eq!(file.state, "删除完成");
        assert_eq!(file.icon, "edit-delete-symbolic");
        assert_progress(file.progress, 1.0);
    }

    #[test]
    fn parses_actual_modified_file_output() {
        let file = parsed("Uploading modified file: .onesync-parser-test/move-target.txt ... done");

        assert_eq!(file.name, ".onesync-parser-test/move-target.txt");
        assert_eq!(file.state, "更新完成");
        assert_eq!(file.icon, "document-save-symbolic");
        assert_progress(file.progress, 1.0);
    }

    #[test]
    fn parses_decimal_upload_progress_output() {
        let file = parsed(
            "Uploading: ./.onesync-progress-test/upload-progress.bin ... 37.5%  |  ETA    00:00:10",
        );

        assert_eq!(file.state, "正在上传");
        assert_progress(file.progress, 0.375);
    }

    #[test]
    fn starts_incomplete_transfer_at_zero_without_percent() {
        let file = parsed("Uploading: ./.onesync-progress-test/upload-progress.bin");

        assert_eq!(file.state, "正在上传");
        assert_progress(file.progress, 0.0);
    }

    #[test]
    fn live_upload_new_file_has_semantic_kind() {
        let file = parsed("Uploading new file: ./docs/a.txt ... done");

        assert_eq!(file.name, "./docs/a.txt");
        assert_eq!(file.kind, TransferKind::UploadNew);
        assert_eq!(file.direction, TransferDirection::LocalToRemote);
        assert_eq!(file.state, "上传完成");
        assert_progress(file.progress, 1.0);
    }

    #[test]
    fn live_remote_delete_has_remote_delete_kind() {
        let file = parsed("Deleting item from Microsoft OneDrive: docs/a.txt");

        assert_eq!(file.kind, TransferKind::DeleteRemote);
        assert_eq!(file.direction, TransferDirection::LocalToRemote);
        assert_eq!(file.state, "删除完成");
    }

    #[test]
    fn live_local_delete_has_local_delete_kind() {
        let file = parsed("Deleting local file: docs/a.txt");

        assert_eq!(file.kind, TransferKind::DeleteLocal);
        assert_eq!(file.direction, TransferDirection::RemoteToLocal);
        assert_eq!(file.state, "删除完成");
    }

    #[test]
    fn preview_upload_new_file_reuses_transfer_parser_without_completion_state() {
        let change = parse_preview_line("Uploading new file: ./docs/a.txt ... done")
            .expect("dry-run upload should become a preview change");

        assert_eq!(change.id, "upload-new:./docs/a.txt");
        assert_eq!(change.path, "./docs/a.txt");
        assert_eq!(change.kind, TransferKind::UploadNew);
        assert_eq!(change.apply, PreviewApply::UploadLocalToRemote);
        assert_eq!(change.intent, PreviewIntent::LocalChangeToRemote);
        assert_eq!(change.state, PreviewState::Pending);
        assert_eq!(change.description, "将上传到 OneDrive");
    }

    #[test]
    fn preview_download_file_maps_to_graph_download() {
        let change = parse_preview_line("Downloading file: docs/remote.txt ... done")
            .expect("dry-run download should become a preview change");

        assert_eq!(change.id, "download:docs/remote.txt");
        assert_eq!(change.kind, TransferKind::Download);
        assert_eq!(change.apply, PreviewApply::DownloadRemoteToLocal);
        assert_eq!(change.intent, PreviewIntent::RemoteChangeToLocal);
        assert_eq!(change.intent.label(), "远端新增或更新");
        assert_eq!(change.description, "将下载到本地");
    }

    #[test]
    fn preview_download_marks_cli_dry_run_basis_and_exact_confidence() {
        let change = parse_preview_line("Downloading file: docs/remote.txt ... done")
            .expect("dry-run download should become a preview change");

        assert_eq!(change.basis, PreviewBasis::CliDryRun);
        assert_eq!(change.confidence, PreviewConfidence::Exact);
        assert_eq!(change.intent, PreviewIntent::RemoteChangeToLocal);
    }

    #[test]
    fn preview_delete_remote_marks_cli_dry_run_basis_and_exact_confidence() {
        let change =
            parse_preview_line("Deleting item from Microsoft OneDrive: docs/local-gone.txt")
                .expect("remote delete preview should be parsed");

        assert_eq!(change.basis, PreviewBasis::CliDryRun);
        assert_eq!(change.confidence, PreviewConfidence::Exact);
        assert_eq!(change.intent, PreviewIntent::LocalDeleteToRemote);
    }

    #[test]
    fn preview_remote_delete_means_local_delete_was_recognized() {
        let change =
            parse_preview_line("Deleting item from Microsoft OneDrive: docs/local-gone.txt")
                .expect("remote delete preview should be parsed");

        assert_eq!(change.id, "delete-remote:docs/local-gone.txt");
        assert_eq!(change.kind, TransferKind::DeleteRemote);
        assert_eq!(change.apply, PreviewApply::DeleteRemote);
        assert_eq!(change.intent, PreviewIntent::LocalDeleteToRemote);
        assert_eq!(change.intent.label(), "本地删除");
    }

    #[test]
    fn preview_local_delete_means_remote_delete_was_recognized() {
        let change = parse_preview_line("Deleting local file: docs/remote-gone.txt")
            .expect("local delete preview should be parsed");

        assert_eq!(change.id, "delete-local:docs/remote-gone.txt");
        assert_eq!(change.kind, TransferKind::DeleteLocal);
        assert_eq!(change.apply, PreviewApply::DeleteLocal);
        assert_eq!(change.intent, PreviewIntent::RemoteDeleteToLocal);
        assert_eq!(change.intent.label(), "远端删除");
    }

    #[test]
    fn preview_ignores_dry_run_housekeeping_lines() {
        assert!(
            parse_preview_line(
                "DRY-RUN Configured. Output below shows what 'would' have occurred."
            )
            .is_none()
        );
        assert!(
            parse_preview_line("DRY RUN: Not updating hash files as --dry-run has been used")
                .is_none()
        );
        assert!(parse_preview_line("Sync with Microsoft OneDrive is complete").is_none());
    }

    #[test]
    fn preview_move_line_captures_source_and_target_paths() {
        let change = parse_preview_line("Moving file: docs/old.txt -> archive/new.txt ... done")
            .expect("move preview should be parsed");

        assert_eq!(change.kind, TransferKind::Move);
        assert_eq!(change.path, "archive/new.txt");
        assert_eq!(change.source_path.as_deref(), Some("docs/old.txt"));
        assert_eq!(change.apply, PreviewApply::MoveRemoteItem);
    }

    #[test]
    fn ignores_status_and_scan_output() {
        assert!(parse_transfer_line("Configuration file successfully loaded").is_none());
        assert!(parse_transfer_line("Processing: .onesync-parser-test/move-target.txt").is_none());
        assert!(parse_transfer_line("Uploading filesystem metadata cache").is_none());
        assert!(parse_transfer_line("Downloading filesystem metadata cache").is_none());
        assert!(parse_transfer_line("Deleting itemized sync database entry").is_none());
        assert!(parse_transfer_line("The file has been deleted locally").is_none());
        assert!(parse_transfer_line("Sync with Microsoft OneDrive is complete").is_none());
    }
}

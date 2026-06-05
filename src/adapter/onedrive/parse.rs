use crate::event::payload::{
    ChangeDirection, ChangeKind, FileChange, PreviewAction, PreviewChange, PreviewIntent,
    PreviewState,
};

#[derive(Clone, Copy)]
struct ChangePattern {
    prefix: &'static str,
    kind: ChangeKind,
    direction: ChangeDirection,
    complete_without_done: bool,
}

const fn pattern(
    prefix: &'static str,
    kind: ChangeKind,
    direction: ChangeDirection,
    complete_without_done: bool,
) -> ChangePattern {
    ChangePattern {
        prefix,
        kind,
        direction,
        complete_without_done,
    }
}


const CHANGE_PATTERNS: &[ChangePattern] = &[
    pattern(
        "Downloading file:",
        ChangeKind::Download,
        ChangeDirection::RemoteToLocal,
        false,
    ),
    pattern(
        "Downloading file",
        ChangeKind::Download,
        ChangeDirection::RemoteToLocal,
        false,
    ),
    pattern(
        "Downloading:",
        ChangeKind::Download,
        ChangeDirection::RemoteToLocal,
        false,
    ),
    pattern(
        "Uploading:",
        ChangeKind::UploadNew,
        ChangeDirection::LocalToRemote,
        false,
    ),
    pattern(
        "Uploading modified file:",
        ChangeKind::UploadModified,
        ChangeDirection::LocalToRemote,
        false,
    ),
    pattern(
        "Uploading modified file",
        ChangeKind::UploadModified,
        ChangeDirection::LocalToRemote,
        false,
    ),
    pattern(
        "Uploading new file:",
        ChangeKind::UploadNew,
        ChangeDirection::LocalToRemote,
        false,
    ),
    pattern(
        "Uploading new file",
        ChangeKind::UploadNew,
        ChangeDirection::LocalToRemote,
        false,
    ),
    pattern(
        "Uploading file:",
        ChangeKind::UploadNew,
        ChangeDirection::LocalToRemote,
        false,
    ),
    pattern(
        "Uploading file",
        ChangeKind::UploadNew,
        ChangeDirection::LocalToRemote,
        false,
    ),
    pattern(
        "Deleting item from Microsoft OneDrive:",
        ChangeKind::DeleteRemote,
        ChangeDirection::LocalToRemote,
        true,
    ),
    pattern(
        "Deleting local file:",
        ChangeKind::DeleteLocal,
        ChangeDirection::RemoteToLocal,
        true,
    ),
    pattern(
        "Deleting remote file:",
        ChangeKind::DeleteRemote,
        ChangeDirection::LocalToRemote,
        true,
    ),
    pattern(
        "Deleting file:",
        ChangeKind::DeleteRemote,
        ChangeDirection::LocalToRemote,
        true,
    ),
    pattern(
        "Deleting local item:",
        ChangeKind::DeleteLocal,
        ChangeDirection::RemoteToLocal,
        true,
    ),
    pattern(
        "Deleting remote item:",
        ChangeKind::DeleteRemote,
        ChangeDirection::LocalToRemote,
        true,
    ),
    pattern(
        "Deleting item:",
        ChangeKind::DeleteRemote,
        ChangeDirection::LocalToRemote,
        true,
    ),
    pattern(
        "Moving file:",
        ChangeKind::Move,
        ChangeDirection::RemoteMetadata,
        false,
    ),
    pattern(
        "Renaming file:",
        ChangeKind::Rename,
        ChangeDirection::RemoteMetadata,
        false,
    ),
];

pub fn parse_file_change_line(line: &str) -> Option<FileChange> {
    let trimmed = line.trim();
    let (pattern, mut path) = CHANGE_PATTERNS
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

    Some(FileChange {
        name: path.to_string(),
        state,
        progress,
        icon: pattern.kind.icon_name(),
        kind: pattern.kind,
        direction: pattern.direction,
    })
}

pub fn parse_preview_change_line(line: &str) -> Option<PreviewChange> {
    let file = parse_file_change_line(line)?;
    let (source_path, path) = split_source_target(&file.name)
        .map_or((None, file.name.clone()), |(source, target)| {
            (Some(source.to_string()), target.to_string())
        });
    let apply = preview_apply_for(file.kind, file.direction);
    let intent = preview_intent_for(file.kind, file.direction, apply);
    let id = preview_id(file.kind, &path, source_path.as_deref());

    Some(PreviewChange {
        id,
        path,
        source_path,
        kind: file.kind,
        direction: file.direction,
        apply,
        intent,
        state: PreviewState::Pending,
    })
}

fn preview_apply_for(kind: ChangeKind, direction: ChangeDirection) -> PreviewAction {
    match (kind, direction) {
        (ChangeKind::Download, _) => PreviewAction::DownloadRemoteToLocal,
        (ChangeKind::UploadNew | ChangeKind::UploadModified, _) => {
            PreviewAction::UploadLocalToRemote
        }
        (ChangeKind::DeleteLocal, _) => PreviewAction::DeleteLocal,
        (ChangeKind::DeleteRemote, _) => PreviewAction::DeleteRemote,
        (ChangeKind::Move, _) => PreviewAction::MoveRemoteItem,
        (ChangeKind::Rename, _) => PreviewAction::RenameRemoteItem,
    }
}

fn preview_intent_for(
    kind: ChangeKind,
    direction: ChangeDirection,
    apply: PreviewAction,
) -> PreviewIntent {
    match (kind, direction, apply) {
        (
            ChangeKind::Download,
            ChangeDirection::RemoteToLocal,
            PreviewAction::DownloadRemoteToLocal,
        ) => PreviewIntent::RemoteChangeToLocal,
        (
            ChangeKind::UploadNew | ChangeKind::UploadModified,
            ChangeDirection::LocalToRemote,
            PreviewAction::UploadLocalToRemote,
        ) => PreviewIntent::LocalChangeToRemote,
        (ChangeKind::DeleteRemote, _, PreviewAction::DeleteRemote) => {
            PreviewIntent::LocalDeleteToRemote
        }
        (ChangeKind::DeleteLocal, _, PreviewAction::DeleteLocal) => {
            PreviewIntent::RemoteDeleteToLocal
        }
        (ChangeKind::Move | ChangeKind::Rename, _, _) => PreviewIntent::RemoteMetadataChange,
        _ => PreviewIntent::RemoteChangeToLocal,
    }
}

fn preview_id(kind: ChangeKind, path: &str, source_path: Option<&str>) -> String {
    let prefix = match kind {
        ChangeKind::Download => "download",
        ChangeKind::UploadNew => "upload-new",
        ChangeKind::UploadModified => "upload-modified",
        ChangeKind::DeleteLocal => "delete-local",
        ChangeKind::DeleteRemote => "delete-remote",
        ChangeKind::Move => "move",
        ChangeKind::Rename => "rename",
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
    pattern: &'a ChangePattern,
) -> Option<(&'a ChangePattern, &'a str)> {
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

    fn parsed(line: &str) -> FileChange {
        parse_file_change_line(line).expect("line should be parsed as a transfer event")
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
        assert_eq!(file.kind, ChangeKind::UploadNew);
        assert_eq!(file.direction, ChangeDirection::LocalToRemote);
        assert_eq!(file.state, "上传完成");
        assert_progress(file.progress, 1.0);
    }

    #[test]
    fn live_remote_delete_has_remote_delete_kind() {
        let file = parsed("Deleting item from Microsoft OneDrive: docs/a.txt");

        assert_eq!(file.kind, ChangeKind::DeleteRemote);
        assert_eq!(file.direction, ChangeDirection::LocalToRemote);
        assert_eq!(file.state, "删除完成");
    }

    #[test]
    fn live_local_delete_has_local_delete_kind() {
        let file = parsed("Deleting local file: docs/a.txt");

        assert_eq!(file.kind, ChangeKind::DeleteLocal);
        assert_eq!(file.direction, ChangeDirection::RemoteToLocal);
        assert_eq!(file.state, "删除完成");
    }

    #[test]
    fn preview_upload_new_file_reuses_transfer_parser_without_completion_state() {
        let change = parse_preview_change_line("Uploading new file: ./docs/a.txt ... done")
            .expect("dry-run upload should become a preview change");

        assert_eq!(change.id, "upload-new:./docs/a.txt");
        assert_eq!(change.path, "./docs/a.txt");
        assert_eq!(change.kind, ChangeKind::UploadNew);
        assert_eq!(change.apply, PreviewAction::UploadLocalToRemote);
        assert_eq!(change.intent, PreviewIntent::LocalChangeToRemote);
        assert_eq!(change.state, PreviewState::Pending);
        assert_eq!(change.description(), "将上传到 OneDrive");
    }

    #[test]
    fn preview_download_file_maps_to_graph_download() {
        let change = parse_preview_change_line("Downloading file: docs/remote.txt ... done")
            .expect("dry-run download should become a preview change");

        assert_eq!(change.id, "download:docs/remote.txt");
        assert_eq!(change.kind, ChangeKind::Download);
        assert_eq!(change.apply, PreviewAction::DownloadRemoteToLocal);
        assert_eq!(change.intent, PreviewIntent::RemoteChangeToLocal);
        assert_eq!(change.intent.label(), "远端新增或更新");
        assert_eq!(change.description(), "将下载到本地");
    }

    #[test]
    fn preview_download_exposes_visible_intent_and_description() {
        let change = parse_preview_change_line("Downloading file: docs/remote.txt ... done")
            .expect("dry-run download should become a preview change");

        assert_eq!(change.intent, PreviewIntent::RemoteChangeToLocal);
        assert_eq!(change.intent.label(), "远端新增或更新");
        assert_eq!(change.description(), "将下载到本地");
        assert!(!change.needs_confirmation());
    }

    #[test]
    fn preview_delete_remote_exposes_visible_intent_and_description() {
        let change =
            parse_preview_change_line("Deleting item from Microsoft OneDrive: docs/local-gone.txt")
                .expect("remote delete preview should be parsed");

        assert_eq!(change.intent, PreviewIntent::LocalDeleteToRemote);
        assert_eq!(change.intent.label(), "本地删除");
        assert_eq!(change.description(), "将从 OneDrive 删除");
        assert!(!change.needs_confirmation());
    }

    #[test]
    fn preview_remote_delete_means_local_delete_was_recognized() {
        let change =
            parse_preview_change_line("Deleting item from Microsoft OneDrive: docs/local-gone.txt")
                .expect("remote delete preview should be parsed");

        assert_eq!(change.id, "delete-remote:docs/local-gone.txt");
        assert_eq!(change.kind, ChangeKind::DeleteRemote);
        assert_eq!(change.apply, PreviewAction::DeleteRemote);
        assert_eq!(change.intent, PreviewIntent::LocalDeleteToRemote);
        assert_eq!(change.intent.label(), "本地删除");
    }

    #[test]
    fn preview_local_delete_means_remote_delete_was_recognized() {
        let change = parse_preview_change_line("Deleting local file: docs/remote-gone.txt")
            .expect("local delete preview should be parsed");

        assert_eq!(change.id, "delete-local:docs/remote-gone.txt");
        assert_eq!(change.kind, ChangeKind::DeleteLocal);
        assert_eq!(change.apply, PreviewAction::DeleteLocal);
        assert_eq!(change.intent, PreviewIntent::RemoteDeleteToLocal);
        assert_eq!(change.intent.label(), "远端删除");
    }

    #[test]
    fn preview_ignores_dry_run_housekeeping_lines() {
        assert!(
            parse_preview_change_line(
                "DRY-RUN Configured. Output below shows what 'would' have occurred."
            )
            .is_none()
        );
        assert!(
            parse_preview_change_line("DRY RUN: Not updating hash files as --dry-run has been used")
                .is_none()
        );
        assert!(parse_preview_change_line("Sync with Microsoft OneDrive is complete").is_none());
    }

    #[test]
    fn preview_move_line_captures_source_and_target_paths() {
        let change = parse_preview_change_line("Moving file: docs/old.txt -> archive/new.txt ... done")
            .expect("move preview should be parsed");

        assert_eq!(change.kind, ChangeKind::Move);
        assert_eq!(change.path, "archive/new.txt");
        assert_eq!(change.source_path.as_deref(), Some("docs/old.txt"));
        assert_eq!(change.apply, PreviewAction::MoveRemoteItem);
    }

    #[test]
    fn ignores_status_and_scan_output() {
        assert!(parse_file_change_line("Configuration file successfully loaded").is_none());
        assert!(parse_file_change_line("Processing: .onesync-parser-test/move-target.txt").is_none());
        assert!(parse_file_change_line("Uploading filesystem metadata cache").is_none());
        assert!(parse_file_change_line("Downloading filesystem metadata cache").is_none());
        assert!(parse_file_change_line("Deleting itemized sync database entry").is_none());
        assert!(parse_file_change_line("The file has been deleted locally").is_none());
        assert!(parse_file_change_line("Sync with Microsoft OneDrive is complete").is_none());
    }
}

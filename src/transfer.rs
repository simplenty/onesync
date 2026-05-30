#[derive(Debug, Clone)]
pub struct SyncFile {
    pub name: String,
    pub state: String,
    pub progress: f64,
    pub icon: &'static str,
}

impl SyncFile {
    pub fn is_complete(&self) -> bool {
        self.progress >= 1.0 || self.state.ends_with("完成")
    }

    pub fn is_failed(&self) -> bool {
        self.state.ends_with("失败")
    }
}

#[derive(Clone, Copy)]
struct TransferPattern {
    prefix: &'static str,
    action: &'static str,
    icon: &'static str,
    complete_without_done: bool,
}

const fn pattern(
    prefix: &'static str,
    action: &'static str,
    icon: &'static str,
    complete_without_done: bool,
) -> TransferPattern {
    TransferPattern {
        prefix,
        action,
        icon,
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
    pattern("Downloading file:", "下载", DOWNLOAD_ICON, false),
    pattern("Downloading file", "下载", DOWNLOAD_ICON, false),
    pattern("Downloading:", "下载", DOWNLOAD_ICON, false),
    pattern("Uploading:", "上传", UPLOAD_ICON, false),
    pattern("Uploading modified file:", "更新", UPDATE_ICON, false),
    pattern("Uploading modified file", "更新", UPDATE_ICON, false),
    pattern("Uploading new file:", "上传", UPLOAD_ICON, false),
    pattern("Uploading new file", "上传", UPLOAD_ICON, false),
    pattern("Uploading file:", "上传", UPLOAD_ICON, false),
    pattern("Uploading file", "上传", UPLOAD_ICON, false),
    pattern(
        "Deleting item from Microsoft OneDrive:",
        "删除",
        DELETE_ICON,
        true,
    ),
    pattern("Deleting local file:", "删除", DELETE_ICON, true),
    pattern("Deleting remote file:", "删除", DELETE_ICON, true),
    pattern("Deleting file:", "删除", DELETE_ICON, true),
    pattern("Deleting local item:", "删除", DELETE_ICON, true),
    pattern("Deleting remote item:", "删除", DELETE_ICON, true),
    pattern("Deleting item:", "删除", DELETE_ICON, true),
    pattern("Moving file:", "移动", MOVE_ICON, false),
    pattern("Renaming file:", "重命名", RENAME_ICON, false),
];

pub fn parse_transfer_line(line: &str) -> Option<SyncFile> {
    let trimmed = line.trim();
    let pattern = TRANSFER_PATTERNS
        .iter()
        .find(|pattern| trimmed.starts_with(pattern.prefix))?;

    let mut path = trimmed
        .trim_start_matches(pattern.prefix)
        .trim_start_matches(':')
        .trim();
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

    let state = if failed {
        format!("{}失败", pattern.action)
    } else if done || progress >= 1.0 {
        format!("{}完成", pattern.action)
    } else {
        format!("正在{}", pattern.action)
    };

    Some(SyncFile {
        name: path.to_string(),
        state,
        progress,
        icon: pattern.icon,
    })
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
    fn ignores_status_and_scan_output() {
        assert!(parse_transfer_line("Configuration file successfully loaded").is_none());
        assert!(parse_transfer_line("Processing: .onesync-parser-test/move-target.txt").is_none());
        assert!(parse_transfer_line("The file has been deleted locally").is_none());
        assert!(parse_transfer_line("Sync with Microsoft OneDrive is complete").is_none());
    }
}

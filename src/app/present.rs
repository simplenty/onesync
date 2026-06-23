use crate::event::{
    BackendError, ClientCheck, ConfirmationKind, ProcPhase,
    payload::{ChangeKind, FileChange, PreviewAction, PreviewChange, PreviewIntent},
};
use crate::operation::{AccountOperation, OperationKind, OperationPhase, controls::ControlAction};
use crate::profile::{Account, AccountStatus};

pub(in crate::app) fn backend_error_message(error: &BackendError) -> String {
    match error {
        BackendError::AuthExpired => "认证已过期或缺少 refresh_token，请重新完成登录".to_string(),
        BackendError::AuthFailed => "认证失败，请重新完成该账户登录".to_string(),
        BackendError::Network => "网络连接失败，请检查网络或代理后重试".to_string(),
        BackendError::UnsupportedConfig => {
            "配置文件包含 onedrive 不支持的选项，请编辑账户配置".to_string()
        }
        BackendError::PartialTransfer => "部分上传或下载失败，请查看传输列表和详情".to_string(),
        BackendError::CliCrashed => "同步工具异常退出，请升级同步工具或检查该账户配置".to_string(),
        BackendError::CliOutput(line) => {
            if line.trim().is_empty() {
                "onedrive 操作失败".to_string()
            } else {
                line.clone()
            }
        }
        BackendError::SpawnFailed(detail) => format!("无法启动认证: {detail}"),
        BackendError::WaitFailed(phase, detail) => {
            format!("等待{}进程失败: {detail}", proc_phase_label(*phase))
        }
        BackendError::MonitorInaccessible => "无法访问持续同步进程".to_string(),
        BackendError::MonitorPollFailed(detail) => format!("轮询持续同步进程失败: {detail}"),
        BackendError::ApplyFailed(detail) => format!("应用失败：{detail}"),
        BackendError::ReconcileFailed(detail) => format!("同步状态更新失败：{detail}"),
        BackendError::IdentityLookupFailed(detail) => {
            format!("无法读取 Microsoft 账号信息: {detail}")
        }
        BackendError::DuplicateAccountName => "账户名称已存在".to_string(),
        BackendError::DuplicateSyncDir => "同步目录已存在".to_string(),
        BackendError::ProfileCreateFailed(detail) => {
            format!("写入账户文件失败: {detail}")
        }
    }
}

pub(in crate::app) fn proc_phase_label(phase: ProcPhase) -> &'static str {
    match phase {
        ProcPhase::Auth => "认证",
        ProcPhase::Preview => "预览",
        ProcPhase::Sync => "同步",
    }
}

// ── ChangeKind → UI strings ──

pub(in crate::app) fn change_kind_icon(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Download => "go-down-symbolic",
        ChangeKind::UploadNew => "go-up-symbolic",
        ChangeKind::UploadModified => "document-save-symbolic",
        ChangeKind::DeleteLocal | ChangeKind::DeleteRemote => "edit-delete-symbolic",
        ChangeKind::Move => "go-jump-symbolic",
        ChangeKind::Rename => "document-edit-symbolic",
    }
}

fn change_kind_label(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Download => "下载",
        ChangeKind::UploadNew => "上传",
        ChangeKind::UploadModified => "更新",
        ChangeKind::DeleteLocal | ChangeKind::DeleteRemote => "删除",
        ChangeKind::Move => "移动",
        ChangeKind::Rename => "重命名",
    }
}

// ── FileChange display ──

pub(in crate::app) fn file_display_state(file: &FileChange) -> String {
    let action = change_kind_label(file.kind);
    if file.failed {
        format!("{action}失败")
    } else if file.is_complete() {
        format!("{action}完成")
    } else {
        format!("正在{action}")
    }
}

// ── PreviewChange strings ──

pub(in crate::app) fn preview_intent_label(intent: PreviewIntent) -> &'static str {
    match intent {
        PreviewIntent::LocalChangeToRemote => "本地新增或修改",
        PreviewIntent::RemoteChangeToLocal => "远端新增或更新",
        PreviewIntent::LocalDeleteToRemote => "本地删除",
        PreviewIntent::RemoteDeleteToLocal => "远端删除",
        PreviewIntent::RemoteMetadataChange => "远端元数据变更",
    }
}

pub(in crate::app) fn preview_intent_detail(intent: PreviewIntent) -> &'static str {
    match intent {
        PreviewIntent::LocalChangeToRemote => "本地内容会同步到 OneDrive，完成后会更新同步状态。",
        PreviewIntent::RemoteChangeToLocal => "OneDrive 内容会下载到本地，完成后会更新同步状态。",
        PreviewIntent::LocalDeleteToRemote => {
            "本地已删除的项目也会从 OneDrive 删除，完成后会更新同步状态。"
        }
        PreviewIntent::RemoteDeleteToLocal => {
            "OneDrive 已删除的项目也会从本地删除，完成后会更新同步状态。"
        }
        PreviewIntent::RemoteMetadataChange => {
            "OneDrive 上的名称或位置变化会同步到本地，完成后会更新同步状态。"
        }
    }
}

pub(in crate::app) fn preview_change_description(change: &PreviewChange) -> &'static str {
    match change.apply {
        PreviewAction::UploadLocalToRemote => "将上传到 OneDrive",
        PreviewAction::DownloadRemoteToLocal => "将下载到本地",
        PreviewAction::DeleteRemote => "将从 OneDrive 删除",
        PreviewAction::DeleteLocal => "将从本地删除",
        PreviewAction::MoveRemoteItem => "将在 OneDrive 移动",
        PreviewAction::RenameRemoteItem => "将在 OneDrive 重命名",
    }
}

// ── Backend event strings ──

pub(in crate::app) fn client_check_message(check: &ClientCheck) -> String {
    match check {
        ClientCheck::Unknown => "正在检测同步工具".to_string(),
        ClientCheck::Ready(version) => format!(
            "同步工具 {}.{}.{} 可用",
            version.major, version.minor, version.patch
        ),
        ClientCheck::Missing(error) => format!("未找到同步工具: {error}"),
        ClientCheck::Unsupported { found, minimum } => format!(
            "同步工具版本过低: 当前 {}.{}.{}, 需要 >= {}.{}.{}",
            found.major, found.minor, found.patch, minimum.major, minimum.minor, minimum.patch
        ),
    }
}

pub(in crate::app) fn confirmation_kind_message(kind: ConfirmationKind) -> &'static str {
    match kind {
        ConfirmationKind::ResyncRequired => {
            "onedrive 要求执行 --resync。请确认该账户的本地与远端状态后再手动处理。"
        }
        ConfirmationKind::BigDelete => {
            "onedrive 检测到大量删除，需要授权。请先检查删除列表后再继续。"
        }
        ConfirmationKind::DownloadOnlyCleanup => {
            "download-only 清理可能删除本地文件。请确认配置后再继续。"
        }
        ConfirmationKind::UploadOnlyNoRemoteDelete => {
            "upload-only 与 no-remote-delete 组合需要显式确认兼容性。"
        }
    }
}

// ── Operation state strings ──

pub(in crate::app) fn operation_label(op: AccountOperation) -> &'static str {
    match (op.kind, op.phase) {
        (OperationKind::Authentication, OperationPhase::Running) => "认证",
        (OperationKind::OneTimeSync, OperationPhase::Running) => "一次同步",
        (OperationKind::Preview, OperationPhase::Running) => "预览",
        (OperationKind::Monitor, OperationPhase::Running) => "持续同步",
        (_, OperationPhase::Stopping) => "停止",
    }
}

// ── ControlAction → GTK strings ──

pub(in crate::app) fn control_action_icon(action: ControlAction) -> &'static str {
    match action {
        ControlAction::StartManualSync => "view-refresh-symbolic",
        ControlAction::StartMonitor => "media-playback-start-symbolic",
        ControlAction::StartPreview => "view-list-symbolic",
        ControlAction::Stop => "media-playback-stop-symbolic",
        ControlAction::Stopping => "process-stop-symbolic",
    }
}

pub(in crate::app) fn control_action_label(action: ControlAction) -> &'static str {
    match action {
        ControlAction::StartManualSync => "同步",
        ControlAction::StartMonitor => "自动同步",
        ControlAction::StartPreview => "预览",
        ControlAction::Stop => "停止",
        ControlAction::Stopping => "正在停止",
    }
}

pub(in crate::app) fn status_title(status: &AccountStatus) -> &'static str {
    match status {
        AccountStatus::NeedsAuth => "需要认证",
        AccountStatus::Authenticated => "已认证",
        AccountStatus::Error(_) => "需要处理",
    }
}

pub(in crate::app) fn status_label(status: &AccountStatus) -> &str {
    match status {
        AccountStatus::NeedsAuth => "未认证",
        AccountStatus::Authenticated => "已认证",
        AccountStatus::Error(message) => message.as_str(),
    }
}

pub(in crate::app) fn status_detail(account: &Account) -> String {
    match &account.status {
        AccountStatus::NeedsAuth => format!("配置目录: {}", account.config_dir),
        AccountStatus::Authenticated => format!("同步目录: {}", account.sync_dir),
        AccountStatus::Error(message) => format!("最近错误: {message}"),
    }
}

pub(in crate::app) fn account_label(account: &Account) -> String {
    if account.email.trim().is_empty() {
        account.id.clone()
    } else {
        account.email.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_every_error_variant() {
        assert_eq!(
            backend_error_message(&BackendError::AuthExpired),
            "认证已过期或缺少 refresh_token，请重新完成登录"
        );
        assert_eq!(
            backend_error_message(&BackendError::CliOutput(String::new())),
            "onedrive 操作失败"
        );
        assert_eq!(
            backend_error_message(&BackendError::CliOutput("last line".to_string())),
            "last line"
        );
        assert_eq!(
            backend_error_message(&BackendError::WaitFailed(
                ProcPhase::Preview,
                "boom".to_string()
            )),
            "等待预览进程失败: boom"
        );
        assert_eq!(
            backend_error_message(&BackendError::DuplicateSyncDir),
            "同步目录已存在"
        );
    }

    #[test]
    fn proc_phase_labels_in_chinese() {
        assert_eq!(proc_phase_label(ProcPhase::Auth), "认证");
        assert_eq!(proc_phase_label(ProcPhase::Preview), "预览");
        assert_eq!(proc_phase_label(ProcPhase::Sync), "同步");
    }
}

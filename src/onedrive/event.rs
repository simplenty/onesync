use crate::transfer::{PreviewChange, SyncFile};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Version {
    pub(crate) major: u64,
    pub(crate) minor: u64,
    pub(crate) patch: u64,
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
            Self::Unknown => "正在检测同步工具".to_string(),
            Self::Ready(version) => format!(
                "同步工具 {}.{}.{} 可用",
                version.major, version.minor, version.patch
            ),
            Self::Missing(error) => format!("未找到同步工具: {error}"),
            Self::Unsupported { found, minimum } => format!(
                "同步工具版本过低: 当前 {}.{}.{}, 需要 >= {}.{}.{}",
                found.major, found.minor, found.patch, minimum.major, minimum.minor, minimum.patch
            ),
        }
    }
}

#[derive(Debug)]
#[allow(dead_code)]
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
    AccountIdentityFound {
        account_id: String,
        display_name: Option<String>,
        email: Option<String>,
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
    TransferEvent {
        account_id: String,
        file: SyncFile,
    },
    PreviewEvent {
        account_id: String,
        change: PreviewChange,
    },
    PreviewFinished {
        account_id: String,
        success: bool,
        requested_stop: bool,
        auth_required: bool,
        message: Option<String>,
        requires_confirmation: Option<ConfirmationKind>,
    },
    PreviewApplyFinished {
        account_id: String,
        change_id: String,
        success: bool,
        message: Option<String>,
    },
    PreviewApplyProgress {
        account_id: String,
        change_id: String,
        progress: f64,
    },
    PreviewReconcileStarted {
        account_id: String,
        change_id: String,
        scope: String,
    },
    PreviewReconcileFinished {
        account_id: String,
        change_id: String,
        success: bool,
        message: Option<String>,
    },
    ConfirmationRequired {
        account_id: String,
        kind: ConfirmationKind,
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
                "onedrive 要求执行 --resync。请确认该账户的本地与远端状态后再手动处理。"
            }
            Self::BigDelete => "onedrive 检测到大量删除，需要授权。请先检查删除列表后再继续。",
            Self::DownloadOnlyCleanup => "download-only 清理可能删除本地文件。请确认配置后再继续。",
            Self::UploadOnlyNoRemoteDelete => {
                "upload-only 与 no-remote-delete 组合需要显式确认兼容性。"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    Download,
    UploadNew,
    UploadModified,
    DeleteLocal,
    DeleteRemote,
    Move,
    Rename,
}

impl ChangeKind {
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
pub enum ChangeDirection {
    LocalToRemote,
    RemoteToLocal,
    RemoteMetadata,
}

#[derive(Debug, Clone)]
pub struct FileChange {
    pub name: String,
    pub state: String,
    pub progress: f64,
    pub icon: &'static str,
    pub kind: ChangeKind,
    pub direction: ChangeDirection,
}

impl FileChange {
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
pub enum PreviewAction {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewChange {
    pub id: String,
    pub path: String,
    pub source_path: Option<String>,
    pub kind: ChangeKind,
    pub direction: ChangeDirection,
    pub apply: PreviewAction,
    pub intent: PreviewIntent,
    pub state: PreviewState,
}

impl PreviewChange {
    #[must_use]
    pub fn description(&self) -> &'static str {
        preview_description(self.apply)
    }

    #[must_use]
    pub fn icon_name(&self) -> &'static str {
        self.kind.icon_name()
    }

    #[must_use]
    pub fn needs_confirmation(&self) -> bool {
        matches!(self.intent, PreviewIntent::AmbiguousRemoteToLocal)
    }
}

const DOWNLOAD_ICON: &str = "go-down-symbolic";
const UPLOAD_ICON: &str = "go-up-symbolic";
const UPDATE_ICON: &str = "document-save-symbolic";
const DELETE_ICON: &str = "edit-delete-symbolic";
const MOVE_ICON: &str = "go-jump-symbolic";
const RENAME_ICON: &str = "document-edit-symbolic";

fn preview_description(apply: PreviewAction) -> &'static str {
    match apply {
        PreviewAction::UploadLocalToRemote => "将上传到 OneDrive",
        PreviewAction::DownloadRemoteToLocal => "将下载到本地",
        PreviewAction::DeleteRemote => "将从 OneDrive 删除",
        PreviewAction::DeleteLocal => "将从本地删除",
        PreviewAction::MoveRemoteItem => "将在 OneDrive 移动",
        PreviewAction::RenameRemoteItem => "将在 OneDrive 重命名",
    }
}

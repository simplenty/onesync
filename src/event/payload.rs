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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeDirection {
    LocalToRemote,
    RemoteToLocal,
    RemoteMetadata,
}

#[derive(Debug, Clone)]
pub struct FileChange {
    pub name: String,
    pub progress: f64,
    pub failed: bool,
    pub kind: ChangeKind,
    pub direction: ChangeDirection,
}

impl FileChange {
    pub fn is_complete(&self) -> bool {
        self.progress >= 1.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreviewState {
    Pending,
    Applying,
    Reconciling,
    Failed,
    ReconcileFailed,
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
pub enum PreviewIntent {
    LocalChangeToRemote,
    RemoteChangeToLocal,
    LocalDeleteToRemote,
    RemoteDeleteToLocal,
    RemoteMetadataChange,
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

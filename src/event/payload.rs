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

impl ChangeKind {}

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

    pub fn is_failed(&self) -> bool {
        self.failed
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

impl PreviewIntent {}

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
    pub fn needs_confirmation(&self) -> bool {
        matches!(self.intent, PreviewIntent::AmbiguousRemoteToLocal)
    }
}

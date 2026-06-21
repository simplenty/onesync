use crate::event::error::{BackendError, ProcPhase};
use crate::event::payload::{FileChange, PreviewChange};

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
        error: Option<BackendError>,
    },
    AccountIdentityFound {
        account_id: String,
        display_name: Option<String>,
        email: Option<String>,
        error: Option<BackendError>,
    },
    SyncFinished {
        account_id: String,
        success: bool,
        requested_stop: bool,
        auth_required: bool,
        error: Option<BackendError>,
        requires_confirmation: Option<ConfirmationKind>,
    },
    TransferEvent {
        account_id: String,
        file: FileChange,
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
        error: Option<BackendError>,
        requires_confirmation: Option<ConfirmationKind>,
    },
    PreviewApplyFinished {
        account_id: String,
        change_id: String,
        success: bool,
        error: Option<BackendError>,
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
        error: Option<BackendError>,
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
        error: Option<BackendError>,
        requires_confirmation: Option<ConfirmationKind>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationKind {
    ResyncRequired,
    BigDelete,
    DownloadOnlyCleanup,
    UploadOnlyNoRemoteDelete,
}

impl ConfirmationKind {}

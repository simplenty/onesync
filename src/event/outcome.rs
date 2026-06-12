use super::{BackendError, ConfirmationKind};

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationOutcome {
    pub success: bool,
    pub requested_stop: bool,
    pub auth_required: bool,
    pub error: Option<BackendError>,
    pub requires_confirmation: Option<ConfirmationKind>,
}

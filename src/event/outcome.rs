use super::ConfirmationKind;

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationOutcome {
    pub success: bool,
    pub requested_stop: bool,
    pub auth_required: bool,
    pub message: Option<String>,
    pub requires_confirmation: Option<ConfirmationKind>,
}

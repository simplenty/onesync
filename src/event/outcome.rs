use super::{BackendError, ConfirmationKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationOutcome {
    pub success: bool,
    pub requested_stop: bool,
    pub auth_required: bool,
    pub error: Option<BackendError>,
    pub requires_confirmation: Option<ConfirmationKind>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::ConfirmationKind;

    #[test]
    fn success_outcome_has_no_error_or_confirmation() {
        let outcome = OperationOutcome {
            success: true,
            requested_stop: false,
            auth_required: false,
            error: None,
            requires_confirmation: None,
        };
        assert!(outcome.success);
        assert!(outcome.error.is_none());
        assert!(outcome.requires_confirmation.is_none());
    }

    #[test]
    fn confirmation_outcome_carries_kind() {
        let outcome = OperationOutcome {
            success: false,
            requested_stop: false,
            auth_required: false,
            error: None,
            requires_confirmation: Some(ConfirmationKind::BigDelete),
        };
        assert_eq!(
            outcome.requires_confirmation,
            Some(ConfirmationKind::BigDelete)
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationKind {
    Authentication,
    OneTimeSync,
    Preview,
    Monitor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationPhase {
    Running,
    Stopping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AccountOperation {
    pub kind: OperationKind,
    pub phase: OperationPhase,
}

#[allow(dead_code)]
impl AccountOperation {
    #[must_use]
    pub fn running(kind: OperationKind) -> Self {
        Self {
            kind,
            phase: OperationPhase::Running,
        }
    }

    #[must_use]
    pub fn stopping(kind: OperationKind) -> Self {
        Self {
            kind,
            phase: OperationPhase::Stopping,
        }
    }

    #[must_use]
    pub fn is_stopping(self) -> bool {
        matches!(self.phase, OperationPhase::Stopping)
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationKind {
    Authentication,
    OneTimeSync,
    Preview,
    Monitor,
    ApplyPreviewChange,
    Reconcile,
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
        Self { kind, phase: OperationPhase::Running }
    }

    #[must_use]
    pub fn stopping(kind: OperationKind) -> Self {
        Self { kind, phase: OperationPhase::Stopping }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match (self.kind, self.phase) {
            (OperationKind::Authentication, OperationPhase::Running) => "认证",
            (OperationKind::OneTimeSync, OperationPhase::Running) => "一次同步",
            (OperationKind::Preview, OperationPhase::Running) => "预览",
            (OperationKind::Monitor, OperationPhase::Running) => "持续同步",
            (OperationKind::ApplyPreviewChange, OperationPhase::Running) => "应用变更",
            (OperationKind::Reconcile, OperationPhase::Running) => "更新同步状态",
            (_, OperationPhase::Stopping) => "停止",
        }
    }

    #[must_use]
    pub fn is_stopping(self) -> bool {
        matches!(self.phase, OperationPhase::Stopping)
    }
}

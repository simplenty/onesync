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

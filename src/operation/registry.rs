use super::{AccountOperation, OperationKind, OperationPhase};
use std::collections::HashMap;

#[derive(Default)]
pub struct OperationRegistry {
    operations: HashMap<String, AccountOperation>,
}

impl OperationRegistry {
    #[must_use]
    pub fn get(&self, profile_id: &str) -> Option<AccountOperation> {
        self.operations.get(profile_id).copied()
    }

    pub fn begin(&mut self, profile_id: impl Into<String>, kind: OperationKind) -> bool {
        let profile_id = profile_id.into();
        if self.operations.contains_key(&profile_id) {
            return false;
        }
        self.operations.insert(
            profile_id,
            AccountOperation {
                kind,
                phase: OperationPhase::Running,
            },
        );
        true
    }

    pub fn mark_stopping(&mut self, profile_id: &str) {
        if let Some(operation) = self.operations.get_mut(profile_id) {
            operation.phase = OperationPhase::Stopping;
        }
    }

    pub fn finish(&mut self, profile_id: &str) {
        self.operations.remove(profile_id);
    }

    #[must_use]
    pub fn contains(&self, profile_id: &str) -> bool {
        self.operations.contains_key(profile_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_inserts_running_operation() {
        let mut registry = OperationRegistry::default();
        assert!(registry.begin("a", OperationKind::OneTimeSync));
        let op = registry.get("a").unwrap();
        assert_eq!(op.kind, OperationKind::OneTimeSync);
        assert_eq!(op.phase, OperationPhase::Running);
    }

    #[test]
    fn begin_rejects_duplicate_profile() {
        let mut registry = OperationRegistry::default();
        assert!(registry.begin("a", OperationKind::Monitor));
        assert!(!registry.begin("a", OperationKind::OneTimeSync));
    }

    #[test]
    fn mark_stopping_transitions_phase() {
        let mut registry = OperationRegistry::default();
        registry.begin("a", OperationKind::OneTimeSync);
        registry.mark_stopping("a");
        assert_eq!(registry.get("a").unwrap().phase, OperationPhase::Stopping);
    }

    #[test]
    fn finish_removes_operation() {
        let mut registry = OperationRegistry::default();
        registry.begin("a", OperationKind::OneTimeSync);
        registry.finish("a");
        assert!(registry.get("a").is_none());
        assert!(!registry.contains("a"));
    }

    #[test]
    fn mark_stopping_and_finish_are_noops_for_unknown_profile() {
        let mut registry = OperationRegistry::default();
        registry.mark_stopping("missing");
        registry.finish("missing");
        assert!(registry.get("missing").is_none());
    }
}

use super::{AccountOperation, OperationKind, OperationPhase};
use std::collections::HashMap;

#[allow(dead_code)]
#[derive(Default)]
pub struct OperationRegistry {
    operations: HashMap<String, AccountOperation>,
}

#[allow(dead_code)]
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
        self.operations.insert(profile_id, AccountOperation { kind, phase: OperationPhase::Running });
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

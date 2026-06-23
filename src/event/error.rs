//! Structured backend error taxonomy.
//!
//! Adapters and the profile layer produce these variants instead of localized
//! strings. The app presenter layer (`app::present`) maps each variant to a
//! user-facing message, so no display text lives outside `src/app`.

/// Which long-running onedrive process produced a wait/poll failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProcPhase {
    Auth,
    Preview,
    Sync,
}

/// Structured failure reported by the backend (onedrive CLI, Graph API, store).
///
/// Carries only raw (non-localized) detail strings; the app presenter converts
/// each variant to a user-facing message.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum BackendError {
    // onedrive CLI output classification
    AuthExpired,
    AuthFailed,
    Network,
    UnsupportedConfig,
    PartialTransfer,
    CliCrashed,
    /// CLI output could not be classified; carries the last non-empty line.
    CliOutput(String),

    // process lifecycle
    /// `onedrive` binary failed to spawn. Carries the raw OS error text.
    SpawnFailed(String),
    /// Waiting on a child process failed. Carries the phase and raw OS error.
    WaitFailed(ProcPhase, String),
    /// The monitor child handle could not be locked.
    MonitorInaccessible,
    /// Polling the monitor child failed. Carries the raw OS error text.
    MonitorPollFailed(String),

    // Graph / preview application
    /// Applying a preview change via Graph failed.
    ApplyFailed(String),
    /// Reconciling sync state after an apply failed.
    ReconcileFailed(String),

    // account identity
    /// Reading the Microsoft account identity failed. Carries the raw error.
    IdentityLookupFailed(String),

    // account store
    DuplicateAccountName,
    DuplicateSyncDir,
    /// Creating the profile config or sync directory failed on disk.
    /// Carries the raw OS error text.
    ProfileCreateFailed(String),
}

#[cfg(test)]
mod tests {
    use super::{BackendError, ProcPhase};

    #[test]
    fn variants_compare_and_carry_payload() {
        assert_eq!(BackendError::AuthExpired, BackendError::AuthExpired);
        assert_ne!(BackendError::AuthExpired, BackendError::AuthFailed);
        assert_eq!(
            BackendError::CliOutput("x".to_string()),
            BackendError::CliOutput("x".to_string())
        );
        assert_eq!(
            BackendError::WaitFailed(ProcPhase::Auth, "e".to_string()),
            BackendError::WaitFailed(ProcPhase::Auth, "e".to_string())
        );
        assert_ne!(
            BackendError::WaitFailed(ProcPhase::Auth, "e".to_string()),
            BackendError::WaitFailed(ProcPhase::Sync, "e".to_string())
        );
    }
}

#![allow(unused_imports)]
pub mod config;
pub mod edit;
pub mod settings;
mod store;

pub use edit::{ProfileEditError, ProfileEditOutcome, save_profile_edit};

pub use config::{
    ConfigEdit, OneDriveConfig, ensure_transfer_metrics_enabled, read_sync_list, write_sync_list,
};
pub use settings::{
    DEFAULT_ONEDRIVE_COMMAND, SyncMode, load_onedrive_command, load_profile_sync_mode,
    remove_profile_sync_mode, save_profile_sync_mode,
};
pub use store::{
    Account, AccountStatus, AccountStore, auth_response_path, auth_url_path, create_account,
    is_authenticated, is_default_profile_name, load_store, remove_confirmation_matches,
    suggested_account_name, suggested_sync_dir,
};

mod event;
mod output;
mod process;

#[allow(unused_imports)]
pub use event::ConfirmationKind;
pub use event::{BackendEvent, ClientCheck};
pub use process::{
    MonitorHandle, SyncHandle, check_client, start_authentication, start_logout, start_monitor,
    start_sync, stop_handle, stop_monitor_handle,
};

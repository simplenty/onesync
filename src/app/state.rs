use super::widgets::TransferList;
use crate::{
    adapter::onedrive::OperationHandle,
    app::tray::{AppTrayHandle, TraySnapshot},
    event::payload::PreviewChange,
    event::{BackendEvent, ClientCheck},
    operation::OperationRegistry,
    profile::SyncMode,
    profile::{Account, AccountStore},
};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::mpsc,
};

pub(in crate::app) struct AppState {
    pub(in crate::app) store: RefCell<AccountStore>,
    pub(in crate::app) selected_index: Cell<usize>,
    pub(in crate::app) client_check: RefCell<ClientCheck>,
    pub(in crate::app) onedrive_command: String,
    pub(in crate::app) sender: mpsc::Sender<BackendEvent>,
    pub(in crate::app) receiver: RefCell<mpsc::Receiver<BackendEvent>>,
    pub(in crate::app) auth_panel: RefCell<Option<AuthPanel>>,
    pub(in crate::app) operation_handles: RefCell<HashMap<String, OperationHandle>>,
    pub(in crate::app) operations: RefCell<OperationRegistry>,
    pub(in crate::app) previews: RefCell<HashMap<String, HashMap<String, PreviewChange>>>,
    pub(in crate::app) applying_preview_changes: RefCell<HashSet<(String, String)>>,
    pub(in crate::app) toast_overlay: adw::ToastOverlay,
    pub(in crate::app) window: adw::ApplicationWindow,
    pub(in crate::app) profile_list: gtk::ListBox,
    pub(in crate::app) title: adw::WindowTitle,
    pub(in crate::app) status_title: gtk::Label,
    pub(in crate::app) status_detail: gtk::Label,
    pub(in crate::app) transfers: TransferList,
    pub(in crate::app) account_menu_button: gtk::MenuButton,
    // ponytail: future extension point per AGENT.md; handler re-added when settings UI lands
    #[allow(dead_code)]
    pub(in crate::app) settings_button: gtk::Button,
    pub(in crate::app) mode_dropdown: gtk::DropDown,
    pub(in crate::app) selected_sync_mode: Cell<SyncMode>,
    pub(in crate::app) updating_sync_mode_dropdown: Cell<bool>,
    pub(in crate::app) sync_button: gtk::Button,
    pub(in crate::app) preview_button: gtk::Button,
    pub(in crate::app) open_sync_dir_button: gtk::Button,
    pub(in crate::app) resync_button: gtk::Button,
    pub(in crate::app) edit_button: gtk::Button,
    pub(in crate::app) auth_button: gtk::Button,
    // ponytail: was RefCell<Option<String>> account id, but only is_some() was read; bool suffices
    pub(in crate::app) pending_confirmation: Cell<bool>,
    pub(in crate::app) tray_handle: RefCell<Option<AppTrayHandle>>,
    pub(in crate::app) tray_snapshot:
        RefCell<Option<std::sync::Arc<std::sync::Mutex<TraySnapshot>>>>,
}

impl AppState {
    pub(in crate::app) fn selected_account(&self) -> Option<Account> {
        self.store
            .borrow()
            .accounts()
            .get(self.selected_index.get())
            .cloned()
    }

    pub(in crate::app) fn selected_account_id(&self) -> Option<String> {
        self.store
            .borrow()
            .accounts()
            .get(self.selected_index.get())
            .map(|account| account.id.clone())
    }

    pub(in crate::app) fn account_by_id(&self, account_id: &str) -> Option<Account> {
        self.store
            .borrow()
            .accounts()
            .iter()
            .find(|account| account.id == account_id)
            .cloned()
    }
}

#[derive(Clone)]
pub(in crate::app) struct AuthPanel {
    pub(in crate::app) account_id: String,
    pub(in crate::app) window: adw::Window,
    pub(in crate::app) status_label: gtk::Label,
    pub(in crate::app) auth_url_entry: gtk::Entry,
    pub(in crate::app) close_button: gtk::Button,
    pub(in crate::app) copy_auth_url_button: gtk::Button,
    pub(in crate::app) finish_button: gtk::Button,
    pub(in crate::app) close_blocked: Rc<Cell<bool>>,
}

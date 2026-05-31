use super::list::TransferList;
use crate::{
    account::Account,
    onedrive::{BackendEvent, ClientCheck, MonitorHandle, SyncHandle},
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::mpsc,
};

pub(in crate::app) struct AppState {
    pub(in crate::app) accounts: RefCell<Vec<Account>>,
    pub(in crate::app) selected_index: Cell<usize>,
    pub(in crate::app) client_check: RefCell<ClientCheck>,
    pub(in crate::app) onedrive_command: String,
    pub(in crate::app) sender: mpsc::Sender<BackendEvent>,
    pub(in crate::app) receiver: RefCell<mpsc::Receiver<BackendEvent>>,
    pub(in crate::app) auth_panel: RefCell<Option<AuthPanel>>,
    pub(in crate::app) syncs: RefCell<HashMap<String, SyncHandle>>,
    pub(in crate::app) monitors: RefCell<HashMap<String, MonitorHandle>>,
    pub(in crate::app) active_operations: RefCell<HashMap<String, ActiveOperation>>,
    pub(in crate::app) toast_overlay: adw::ToastOverlay,
    pub(in crate::app) window: adw::ApplicationWindow,
    pub(in crate::app) profile_list: gtk::ListBox,
    pub(in crate::app) title: adw::WindowTitle,
    pub(in crate::app) status_title: gtk::Label,
    pub(in crate::app) status_detail: gtk::Label,
    pub(in crate::app) transfers: TransferList,
    pub(in crate::app) account_menu_button: gtk::MenuButton,
    pub(in crate::app) settings_button: gtk::Button,
    pub(in crate::app) one_time_sync_button: gtk::Button,
    pub(in crate::app) monitor_button: gtk::Button,
    pub(in crate::app) edit_button: gtk::Button,
}

impl AppState {
    pub(in crate::app) fn selected_account(&self) -> Option<Account> {
        self.accounts
            .borrow()
            .get(self.selected_index.get())
            .cloned()
    }

    pub(in crate::app) fn selected_account_id(&self) -> Option<String> {
        self.accounts
            .borrow()
            .get(self.selected_index.get())
            .map(|account| account.id.clone())
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

#[derive(Clone, Copy)]
pub(in crate::app) enum ActiveOperation {
    Authentication,
    Sync,
    StoppingSync,
    StoppingMonitor,
}

impl ActiveOperation {
    pub(in crate::app) fn label(self) -> &'static str {
        match self {
            Self::Authentication => "认证",
            Self::Sync => "一次同步",
            Self::StoppingSync => "停止同步",
            Self::StoppingMonitor => "停止持续同步",
        }
    }
}

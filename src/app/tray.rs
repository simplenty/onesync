// src/app/tray.rs — system tray via ksni (StatusNotifierItem D-Bus)
// ponytail: single icon "io.github.simplenty.onesync" for all states
//
// Architecture: ksni runs on its own D-Bus thread via blocking::spawn.
// GTK (non-Send Rc) stays on main thread.
// Bridge: TraySnapshot (Send) shared via Arc<Mutex>,
// menu clicks send TrayAction through std::sync::mpsc::Sender →
// GTK idle_add_local handler on main context.

use crate::operation::{OperationKind, OperationPhase};
use crate::profile::{Account, AccountStatus};
use adw::prelude::ApplicationExt;
use std::sync::{Arc, Mutex, mpsc};

// ── Thread-safe snapshot ──

#[derive(Debug, Clone)]
pub(crate) struct TraySnapshot {
    pub(crate) accounts: Vec<TrayAccount>,
}

#[derive(Debug, Clone)]
pub(crate) struct TrayAccount {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) status: TrayAccountStatus,
    pub(crate) operation: Option<TrayOperation>,
}

#[derive(Debug, Clone)]
pub(crate) enum TrayAccountStatus {
    NeedsAuth,
    Authenticated,
    #[allow(dead_code)]
    Error(String),
}

#[derive(Debug, Clone)]
pub(crate) struct TrayOperation {
    #[allow(dead_code)]
    pub(crate) kind: OperationKind,
    pub(crate) phase: OperationPhase,
}

// ── Action channel ──

#[derive(Debug, Clone)]
pub(crate) enum TrayAction {
    Present,
    Quit,
    Sync(String),
    Monitor(String),
    Stop(String),
    Auth(String),
    OpenDir(String),
}

// ── ksni::Tray impl ──

pub(crate) type AppTrayHandle = ksni::blocking::Handle<OneSyncTray>;

pub(super) struct OneSyncTray {
    snapshot: Arc<Mutex<TraySnapshot>>,
    sender: mpsc::Sender<TrayAction>,
}

impl OneSyncTray {
    fn snap(&self) -> TraySnapshot {
        self.snapshot.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl ksni::Tray for OneSyncTray {
    fn id(&self) -> String {
        "io.github.simplenty.onesync".into()
    }

    fn icon_name(&self) -> String {
        "io.github.simplenty.onesync".into()
    }

    fn title(&self) -> String {
        "OneSync".into()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let snap = self.snap();
        ksni::ToolTip {
            icon_name: "io.github.simplenty.onesync".into(),
            icon_pixmap: vec![],
            title: "OneSync".into(),
            description: match snap.accounts.len() {
                0 => "未配置账号".into(),
                n => format!("{n} 个账号"),
            },
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.sender.send(TrayAction::Present);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        let snap = self.snap();

        let mut items: Vec<ksni::MenuItem<Self>> = vec![];

        // ── Open ──
        let sender = self.sender.clone();
        items.push(ksni::MenuItem::Standard(ksni::menu::StandardItem {
            label: "打开 OneSync".into(),
            activate: Box::new(move |_| { let _ = sender.send(TrayAction::Present); }),
            ..Default::default()
        }));

        if !snap.accounts.is_empty() {
            items.push(ksni::MenuItem::Separator);
            for acct in &snap.accounts {
                items.push(build_account_submenu(acct, &self.sender));
            }
        }

        // ── Quit ──
        items.push(ksni::MenuItem::Separator);
        let sender = self.sender.clone();
        items.push(ksni::MenuItem::Standard(ksni::menu::StandardItem {
            label: "退出 OneSync".into(),
            activate: Box::new(move |_| { let _ = sender.send(TrayAction::Quit); }),
            ..Default::default()
        }));

        items
    }
}

fn build_account_submenu(acct: &TrayAccount, sender: &mpsc::Sender<TrayAction>) -> ksni::MenuItem<OneSyncTray> {
    let mut sub: Vec<ksni::MenuItem<OneSyncTray>> = Vec::new();

    match &acct.status {
        TrayAccountStatus::NeedsAuth | TrayAccountStatus::Error(_) => {
            let sender = sender.clone();
            let id = acct.id.clone();
            sub.push(ksni::MenuItem::Standard(ksni::menu::StandardItem {
                label: "重新认证".into(),
                activate: Box::new(move |_| { let _ = sender.send(TrayAction::Auth(id.clone())); }),
                ..Default::default()
            }));
        }
        TrayAccountStatus::Authenticated => {
            sub.push(ksni::MenuItem::Standard(ksni::menu::StandardItem {
                label: "已认证".into(),
                enabled: false,
                ..Default::default()
            }));

            let is_sync = acct.operation.as_ref().is_some_and(|op| op.kind == OperationKind::OneTimeSync);
            let is_monitor = acct.operation.as_ref().is_some_and(|op| op.kind == OperationKind::Monitor);
            let stopping = acct.operation.as_ref().is_some_and(|op| op.phase == OperationPhase::Stopping);
            let has_op = acct.operation.is_some();

            // 手动同步 row — in-place "停止" when OneTimeSync running
            {
                let s = sender.clone();
                let id = acct.id.clone();
                match (is_sync, stopping) {
                    (true, true) => sub.push(ksni::MenuItem::Standard(ksni::menu::StandardItem {
                        label: "正在停止…".into(), enabled: false, ..Default::default()
                    })),
                    (true, false) => sub.push(ksni::MenuItem::Standard(ksni::menu::StandardItem {
                        label: "停止".into(), enabled: true,
                        activate: Box::new(move |_| { let _ = s.send(TrayAction::Stop(id.clone())); }),
                        ..Default::default()
                    })),
                    (false, _) => sub.push(ksni::MenuItem::Standard(ksni::menu::StandardItem {
                        label: "手动同步".into(), enabled: !has_op,
                        activate: Box::new(move |_| { let _ = s.send(TrayAction::Sync(id.clone())); }),
                        ..Default::default()
                    })),
                }
            }

            // 自动同步 row — in-place "停止" when Monitor running
            {
                let s = sender.clone();
                let id = acct.id.clone();
                match (is_monitor, stopping) {
                    (true, true) => sub.push(ksni::MenuItem::Standard(ksni::menu::StandardItem {
                        label: "正在停止…".into(), enabled: false, ..Default::default()
                    })),
                    (true, false) => sub.push(ksni::MenuItem::Standard(ksni::menu::StandardItem {
                        label: "停止".into(), enabled: true,
                        activate: Box::new(move |_| { let _ = s.send(TrayAction::Stop(id.clone())); }),
                        ..Default::default()
                    })),
                    (false, _) => sub.push(ksni::MenuItem::Standard(ksni::menu::StandardItem {
                        label: "自动同步".into(), enabled: !has_op,
                        activate: Box::new(move |_| { let _ = s.send(TrayAction::Monitor(id.clone())); }),
                        ..Default::default()
                    })),
                }
            }
        }
    }

    let sender = sender.clone();
    let id = acct.id.clone();
    sub.push(ksni::MenuItem::Standard(ksni::menu::StandardItem {
        label: "打开同步目录".into(),
        activate: Box::new(move |_| { let _ = sender.send(TrayAction::OpenDir(id.clone())); }),
        ..Default::default()
    }));

    ksni::MenuItem::SubMenu(ksni::menu::SubMenu {
        label: acct.name.clone(),
        submenu: sub,
        ..Default::default()
    })
}

// ── Public API ──

/// Build snapshot from current AppState (called on GTK thread).
pub(super) fn build_snapshot(
    accounts: &[Account],
    ops: &crate::operation::OperationRegistry,
) -> TraySnapshot {
    TraySnapshot {
        accounts: accounts.iter().map(|a| TrayAccount {
            id: a.id.clone(),
            name: a.name.clone(),
            status: match &a.status {
                AccountStatus::NeedsAuth => TrayAccountStatus::NeedsAuth,
                AccountStatus::Authenticated => TrayAccountStatus::Authenticated,
                AccountStatus::Error(msg) => TrayAccountStatus::Error(msg.clone()),
            },
            operation: ops.get(&a.id).map(|op| TrayOperation {
                kind: op.kind,
                phase: op.phase,
            }),
        }).collect(),
    }
}

/// Spawn tray service. Returns handle + shared snapshot Arc + the receiver end of the action channel.
pub(super) fn init() -> (AppTrayHandle, Arc<Mutex<TraySnapshot>>, mpsc::Receiver<TrayAction>) {
    let (sender, receiver) = mpsc::channel();
    let snapshot = Arc::new(Mutex::new(TraySnapshot { accounts: vec![] }));
    let tray = OneSyncTray { snapshot: snapshot.clone(), sender };
    let handle = ksni::blocking::TrayMethods::spawn(tray).expect("failed to spawn ksni tray");
    (handle, snapshot, receiver)
}

/// Push a fresh snapshot and trigger menu rebuild.
pub(super) fn push_snapshot(handle: &AppTrayHandle, arc: &Arc<Mutex<TraySnapshot>>, snap: TraySnapshot) {
    *arc.lock().unwrap_or_else(|e| e.into_inner()) = snap;
    handle.update(|_: &mut OneSyncTray| { /* menu rebuilds on next query — snapshot is shared */ });
}

/// Process a TrayAction on the GTK main thread. `state` must be Rc<AppState>.
pub(super) fn handle_action(action: TrayAction, state: &std::rc::Rc<super::state::AppState>) {
    match action {
        TrayAction::Present => {
            use gtk::prelude::GtkWindowExt;
            state.window.present();
        }
        TrayAction::Quit => {
            super::events::stop_all_monitors(state);
            use gtk::prelude::GtkWindowExt;
            if let Some(app) = state.window.application() {
                app.quit();
            }
        }
        TrayAction::Sync(id) => {
            if let Some(acct) = state.store.borrow().accounts().iter()
                .find(|a| a.id == id).cloned()
            {
                super::actions::start_one_time_sync_for_account(std::rc::Rc::clone(state), acct);
            }
        }
        TrayAction::Monitor(id) => {
            if let Some(acct) = state.store.borrow().accounts().iter()
                .find(|a| a.id == id).cloned()
            {
                super::actions::start_monitor_for_account(std::rc::Rc::clone(state), acct);
            }
        }
        TrayAction::Stop(id) => {
            let kind = state.operations.borrow().get(&id).map(|op| op.kind);
            match kind {
                Some(OperationKind::Monitor) => super::events::stop_monitor(state, &id),
                Some(OperationKind::OneTimeSync | OperationKind::Preview) => {
                    super::events::stop_sync(state, &id, "停止")
                }
                _ => {}
            }
        }
        TrayAction::Auth(id) => {
            if let Some(acct) = state.store.borrow().accounts().iter()
                .find(|a| a.id == id).cloned()
            {
                super::dialogs::auth::show_auth_dialog(std::rc::Rc::clone(state), acct);
            }
        }
        TrayAction::OpenDir(id) => {
            if let Some(acct) = state.store.borrow().accounts().iter()
                .find(|a| a.id == id).cloned()
            {
                super::actions::open_sync_dir_for_account(state, &acct);
            }
        }
    }
}

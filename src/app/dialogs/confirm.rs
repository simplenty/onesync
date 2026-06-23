use super::super::{actions::start_one_time_sync, state::AppState};
use crate::profile::Account;
use adw::prelude::*;
use std::rc::Rc;

pub(in crate::app) fn show_warning_window(state: Rc<AppState>, title: &str, message: &str) {
    let dialog = adw::AlertDialog::new(Some(title), Some(message));
    dialog.add_response("close", "知道了");
    dialog.set_default_response(Some("close"));
    dialog.set_close_response("close");
    let clear_state = Rc::clone(&state);
    dialog.choose(
        Some(&state.window),
        None::<&gtk::gio::Cancellable>,
        move |_| {
            clear_state.pending_confirmation.set(false);
        },
    );
}

pub(in crate::app) fn show_big_delete_confirmation(state: Rc<AppState>, account: Account) {
    let dialog = adw::AlertDialog::new(
        Some("允许大量删除?"),
        Some(
            "onedrive 检测到大量删除。只有在你确认这些删除是预期操作时，才允许继续。\n\n继续后，OneSync 会用 --force 重新运行一次同步，并把本地删除同步到 OneDrive 云端。",
        ),
    );
    dialog.add_responses(&[("cancel", "返回"), ("force", "允许大量删除并重新同步")]);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog.set_response_appearance("force", adw::ResponseAppearance::Destructive);
    let force_state = Rc::clone(&state);
    let force_account = account.clone();
    dialog.choose(
        Some(&state.window),
        None::<&gtk::gio::Cancellable>,
        move |response| {
            if response == "force" {
                start_one_time_sync(Rc::clone(&force_state), force_account.clone(), true, false);
            }
            force_state.pending_confirmation.set(false);
        },
    );
}

pub(in crate::app) fn show_resync_confirmation(state: Rc<AppState>, account: Account) {
    let dialog = adw::AlertDialog::new(
        Some("执行 resync?"),
        Some(
            "onedrive 要求使用 --resync 重建同步状态。\n\n只有在你确认当前本地目录和 OneDrive 云端内容对应同一个账户，并且已理解这可能重新扫描大量文件时，才继续。",
        ),
    );
    dialog.add_responses(&[("cancel", "返回"), ("resync", "执行 resync")]);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog.set_response_appearance("resync", adw::ResponseAppearance::Suggested);
    let resync_state = Rc::clone(&state);
    let resync_account = account.clone();
    dialog.choose(
        Some(&state.window),
        None::<&gtk::gio::Cancellable>,
        move |response| {
            if response == "resync" {
                start_one_time_sync(
                    Rc::clone(&resync_state),
                    resync_account.clone(),
                    false,
                    true,
                );
            }
            resync_state.pending_confirmation.set(false);
        },
    );
}

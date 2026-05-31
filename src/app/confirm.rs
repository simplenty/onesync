use super::{start_forced_one_time_sync_for_account, state::AppState};
use crate::account::Account;
use adw::prelude::*;
use std::rc::Rc;

pub(in crate::app) fn show_warning_window(state: &AppState, title: &str, message: &str) {
    let dialog = adw::AlertDialog::new(Some(title), Some(message));
    dialog.add_response("close", "知道了");
    dialog.set_default_response(Some("close"));
    dialog.set_close_response("close");
    dialog.present(Some(&state.window));
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
                start_forced_one_time_sync_for_account(
                    Rc::clone(&force_state),
                    force_account.clone(),
                );
            }
        },
    );
}

use super::{
    auth::show_auth_dialog,
    can_mutate_profile,
    render::{rebuild_profile_list, refresh_content, show_toast},
    state::AppState,
    status::{account_label, status_label},
    widgets::form_row,
};
use crate::account::{
    Account, create_account, remove_confirmation_matches, save_accounts, suggested_account_name,
    suggested_sync_dir,
};
use adw::prelude::*;
use gtk::Align;
use std::{cell::Cell, rc::Rc};

pub(in crate::app) fn show_add_account_dialog(state: Rc<AppState>) {
    let dialog = adw::Window::builder()
        .title("添加账号")
        .transient_for(&state.window)
        .modal(true)
        .default_width(480)
        .resizable(false)
        .build();

    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();

    let header = adw::HeaderBar::new();
    root.append(&header);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();
    root.append(&content);

    let name_entry = gtk::Entry::builder()
        .placeholder_text("账户名称，例如 个人")
        .text(suggested_account_name())
        .build();
    let sync_dir_entry = gtk::Entry::builder()
        .placeholder_text("本地同步目录")
        .text(suggested_sync_dir())
        .build();

    content.append(&form_row("名称", &name_entry));
    content.append(&form_row("同步目录", &sync_dir_entry));

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .halign(Align::End)
        .margin_top(4)
        .build();
    let cancel_button = gtk::Button::with_label("取消");
    let add_button = gtk::Button::with_label("继续认证");
    add_button.add_css_class("suggested-action");
    actions.append(&cancel_button);
    actions.append(&add_button);
    content.append(&actions);

    let cancel_dialog = dialog.clone();
    cancel_button.connect_clicked(move |_| {
        cancel_dialog.close();
    });

    let dialog_state = Rc::clone(&state);
    let add_dialog = dialog.clone();
    add_button.connect_clicked(move |_| {
        let name = name_entry.text().trim().to_string();
        let sync_dir = sync_dir_entry.text().trim().to_string();
        let account_result = {
            let accounts = dialog_state.accounts.borrow();
            create_account(&accounts, &name, "", &sync_dir)
        };
        match account_result {
            Ok(account) => {
                let auth_account = account.clone();
                let (last_index, save_result) = {
                    let mut accounts = dialog_state.accounts.borrow_mut();
                    accounts.push(account);
                    let last_index = accounts.len().saturating_sub(1);
                    let save_result = save_accounts(&accounts);
                    (last_index, save_result)
                };
                if let Err(error) = save_result {
                    show_toast(&dialog_state, &format!("保存账号失败: {error}"));
                }
                dialog_state.selected_index.set(last_index);
                rebuild_profile_list(&dialog_state);
                refresh_content(&dialog_state);
                dialog_state.transfers.clear();
                show_toast(&dialog_state, "账号已添加");
                add_dialog.close();
                show_auth_dialog(Rc::clone(&dialog_state), auth_account);
            }
            Err(error) => show_toast(&dialog_state, &format!("添加账号失败: {error}")),
        }
    });

    dialog.set_content(Some(&root));
    dialog.present();
}

pub(in crate::app) fn show_edit_profile_dialog(state: Rc<AppState>, account: Account) {
    let dialog = adw::Window::builder()
        .title("编辑账户")
        .transient_for(&state.window)
        .modal(true)
        .default_width(560)
        .build();
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    let header = adw::HeaderBar::new();
    header.set_show_start_title_buttons(false);
    header.set_show_end_title_buttons(false);
    let close_button = gtk::Button::with_label("关闭");
    let save_button = gtk::Button::with_label("保存");
    save_button.add_css_class("suggested-action");
    header.pack_start(&close_button);
    header.pack_end(&save_button);
    root.append(&header);

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    root.append(&stack);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();

    let name_entry = gtk::Entry::builder().text(&account.name).build();
    let email_entry = gtk::Entry::builder()
        .text(account_label(&account))
        .editable(false)
        .build();
    let sync_dir_entry = gtk::Entry::builder()
        .text(&account.sync_dir)
        .editable(false)
        .build();
    let status_entry = gtk::Entry::builder()
        .text(status_label(&account.status))
        .editable(false)
        .build();
    content.append(&form_row("账户名称", &name_entry));
    content.append(&form_row("账号标识", &email_entry));
    content.append(&form_row("同步目录", &sync_dir_entry));
    content.append(&form_row("认证状态", &status_entry));

    let danger_label = gtk::Label::builder()
        .label("危险操作")
        .halign(Align::Start)
        .css_classes(["heading"])
        .margin_top(10)
        .build();
    content.append(&danger_label);

    let danger_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();
    let remove_button = gtk::Button::with_label("移除账户");
    remove_button.add_css_class("destructive-action");
    let can_mutate = can_mutate_profile(&state, &account);
    save_button.set_sensitive(can_mutate);
    remove_button.set_sensitive(can_mutate);
    danger_box.append(&remove_button);
    content.append(&danger_box);

    let remove_content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .hexpand(true)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();

    let remove_title = gtk::Label::builder()
        .label(format!("移除账户 \"{}\"?", account.name))
        .wrap(true)
        .halign(Align::Fill)
        .xalign(0.0)
        .hexpand(true)
        .css_classes(["title-3"])
        .build();
    remove_content.append(&remove_title);

    let risk_label = gtk::Label::builder()
        .label("移除后，这个账号不会再出现在列表里，也不会继续从这个账户启动同步。")
        .wrap(true)
        .halign(Align::Fill)
        .xalign(0.0)
        .hexpand(true)
        .build();
    remove_content.append(&risk_label);

    let guarantee_label = gtk::Label::builder()
        .label("这不会删除 OneDrive 云端文件或本地同步目录。")
        .wrap(true)
        .halign(Align::Fill)
        .xalign(0.0)
        .hexpand(true)
        .css_classes(["dim-label"])
        .build();
    remove_content.append(&guarantee_label);

    let instruction = gtk::Label::builder()
        .label(format!("请输入 \"{}\" 以确认移除。", account.name))
        .wrap(true)
        .halign(Align::Fill)
        .xalign(0.0)
        .hexpand(true)
        .build();
    let remove_name_entry = gtk::Entry::builder()
        .placeholder_text(&account.name)
        .width_chars(32)
        .max_width_chars(48)
        .halign(Align::Fill)
        .hexpand(true)
        .build();
    let confirm_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .hexpand(true)
        .build();
    confirm_box.append(&instruction);
    confirm_box.append(&remove_name_entry);
    remove_content.append(&confirm_box);

    stack.add_named(&content, Some("edit"));
    stack.add_named(&remove_content, Some("remove"));
    stack.set_visible_child_name("edit");

    let remove_mode = Rc::new(Cell::new(false));
    let expected_remove_name = account.name.clone();

    let close_dialog = dialog.clone();
    let close_stack = stack.clone();
    let close_save_button = save_button.clone();
    let close_mode = Rc::clone(&remove_mode);
    close_button.connect_clicked(move |button| {
        if close_mode.get() {
            close_mode.set(false);
            close_stack.set_visible_child_name("edit");
            button.set_label("关闭");
            close_save_button.set_label("保存");
            close_save_button.remove_css_class("destructive-action");
            close_save_button.add_css_class("suggested-action");
            close_save_button.set_sensitive(true);
            return;
        }
        close_dialog.close();
    });

    let save_state = Rc::clone(&state);
    let save_dialog = dialog.clone();
    let save_account_id = account.id.clone();
    let save_mode = Rc::clone(&remove_mode);
    let save_account = account.clone();
    let save_expected_remove_name = expected_remove_name.clone();
    let save_remove_name_entry = remove_name_entry.clone();
    save_button.connect_clicked(move |_| {
        if save_mode.get() {
            if !remove_confirmation_matches(
                &save_expected_remove_name,
                &save_remove_name_entry.text(),
            ) {
                show_toast(&save_state, "请输入完整账户名称");
                return;
            }
            remove_profile(&save_state, &save_account);
            save_dialog.close();
            return;
        }

        let next = name_entry.text().trim().to_string();
        if next.is_empty() {
            show_toast(&save_state, "名称不能为空");
            return;
        }
        if save_state
            .accounts
            .borrow()
            .iter()
            .any(|stored| stored.id != save_account_id && stored.name == next)
        {
            show_toast(&save_state, "账户名称已存在");
            return;
        }
        if let Some(stored) = save_state
            .accounts
            .borrow_mut()
            .iter_mut()
            .find(|stored| stored.id == save_account_id)
        {
            stored.name = next;
        }
        if let Err(error) = save_accounts(&save_state.accounts.borrow()) {
            show_toast(&save_state, &format!("保存账号失败: {error}"));
            return;
        }
        rebuild_profile_list(&save_state);
        refresh_content(&save_state);
        save_dialog.close();
        show_toast(&save_state, "账户已保存");
    });

    let remove_state = Rc::clone(&state);
    let remove_account = account.clone();
    let remove_stack = stack.clone();
    let remove_close_button = close_button.clone();
    let remove_save_button = save_button.clone();
    let remove_mode_for_button = Rc::clone(&remove_mode);
    let remove_entry_for_button = remove_name_entry.clone();
    let expected_remove_name_for_button = expected_remove_name.clone();
    remove_button.connect_clicked(move |_| {
        if !can_mutate_profile(&remove_state, &remove_account) {
            show_toast(&remove_state, "请先停止该账户的认证、同步或持续同步");
            return;
        }
        remove_mode_for_button.set(true);
        remove_entry_for_button.set_text("");
        remove_stack.set_visible_child_name("remove");
        remove_close_button.set_label("返回");
        remove_save_button.set_label("移除账户");
        remove_save_button.remove_css_class("suggested-action");
        remove_save_button.add_css_class("destructive-action");
        remove_save_button.set_sensitive(remove_confirmation_matches(
            &expected_remove_name_for_button,
            &remove_entry_for_button.text(),
        ));
    });

    let confirm_for_entry = save_button.clone();
    let entry_mode = Rc::clone(&remove_mode);
    let expected_name_for_entry = expected_remove_name.clone();
    remove_name_entry.connect_changed(move |entry| {
        if entry_mode.get() {
            confirm_for_entry.set_sensitive(remove_confirmation_matches(
                &expected_name_for_entry,
                &entry.text(),
            ));
        }
    });

    dialog.set_content(Some(&root));
    dialog.present();
}

fn remove_profile(state: &Rc<AppState>, account: &Account) {
    let selected = state.selected_index.get();
    state
        .accounts
        .borrow_mut()
        .retain(|stored| stored.id != account.id);
    state.selected_index.set(selected.saturating_sub(1));
    if let Err(error) = save_accounts(&state.accounts.borrow()) {
        show_toast(state, &format!("保存账号失败: {error}"));
    }
    rebuild_profile_list(state);
    refresh_content(state);
    state.transfers.clear();
}

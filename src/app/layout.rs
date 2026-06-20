use super::{
    dialogs::profile, actions::load_sync_mode_for_selected_profile,
    render::refresh_content,
    state::AppState,
    widgets::{command_button, set_menu_button_content},
};
use adw::prelude::*;
use gtk::Align;
use std::rc::Rc;

pub(in crate::app) const ACCOUNT_CONTEXT_MENU_WIDTH: i32 = 160;

pub(in crate::app) struct ContentWidgets {
    pub(in crate::app) title: adw::WindowTitle,
    pub(in crate::app) status_title: gtk::Label,
    pub(in crate::app) status_detail: gtk::Label,
    pub(in crate::app) files_list: gtk::ListBox,
    pub(in crate::app) account_menu_button: gtk::MenuButton,
    pub(in crate::app) settings_button: gtk::Button,
    pub(in crate::app) mode_dropdown: gtk::DropDown,
    pub(in crate::app) sync_button: gtk::Button,
    pub(in crate::app) preview_button: gtk::Button,
    pub(in crate::app) edit_button: gtk::Button,
    pub(in crate::app) auth_button: gtk::Button,
}

pub(in crate::app) struct ProfileContextPopover {
    pub(in crate::app) popover: gtk::Popover,
    pub(in crate::app) sync_once_button: gtk::Button,
    pub(in crate::app) monitor_button: gtk::Button,
    pub(in crate::app) open_sync_dir_button: gtk::Button,
}

pub(in crate::app) fn build_sidebar(state: Rc<AppState>) -> adw::ToolbarView {
    let header = adw::HeaderBar::new();
    let add_button = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text("添加账号")
        .build();
    header.pack_end(&add_button);

    let title = adw::WindowTitle::builder().title("账户").build();
    header.set_title_widget(Some(&title));

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&header);

    let sidebar_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    sidebar_box.append(&state.profile_list);

    let select_state = Rc::clone(&state);
    state.profile_list.connect_row_selected(move |_, row| {
        let Some(row) = row else {
            return;
        };
        let next_index = row.index() as usize;
        let changed_account = select_state.selected_index.get() != next_index;
        select_state.selected_index.set(next_index);
        load_sync_mode_for_selected_profile(&select_state);
        refresh_content(&select_state);
        if changed_account {
            select_state.transfers.clear();
        }
    });

    let add_state = Rc::clone(&state);
    add_button.connect_clicked(move |_| {
        profile::show_add_account_dialog(Rc::clone(&add_state));
    });

    toolbar_view.set_content(Some(&sidebar_box));
    toolbar_view
}

pub(in crate::app) fn build_content_widgets() -> (adw::ToolbarView, ContentWidgets) {
    let header = adw::HeaderBar::new();
    let title = adw::WindowTitle::builder()
        .title("OneSync")
        .subtitle("未选择账号")
        .build();
    header.set_title_widget(Some(&title));

    let settings_button = gtk::Button::builder()
        .icon_name("emblem-system-symbolic")
        .tooltip_text("设置")
        .build();
    header.pack_end(&settings_button);

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&header);

    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    let summary_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .build();

    let status_line = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();

    let status_title = gtk::Label::builder()
        .label("未配置")
        .halign(Align::Start)
        .hexpand(true)
        .css_classes(["title-3"])
        .build();
    status_line.append(&status_title);

    let status_detail = gtk::Label::builder()
        .label("添加账号后开始认证")
        .halign(Align::Start)
        .wrap(true)
        .css_classes(["dim-label"])
        .build();

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();
    let mode_dropdown = gtk::DropDown::from_strings(&["手动模式", "自动模式"]);
    mode_dropdown.set_selected(super::dropdown_index_from_sync_mode(crate::profile::SyncMode::Manual));
    mode_dropdown.set_tooltip_text(Some("选择自动同步或手动同步"));
    let sync_button = command_button("view-refresh-symbolic", "同步");
    let preview_button = command_button("view-list-symbolic", "预览");
    let edit_button = command_button("document-edit-symbolic", "编辑账户");
    actions.append(&mode_dropdown);
    actions.append(&sync_button);
    actions.append(&preview_button);
    let auth_button = command_button("dialog-password-symbolic", "认证");

    actions.append(&auth_button);
    let account_menu_button = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text("账户操作")
        .build();
    account_menu_button.set_popover(Some(&build_account_actions_popover(&[(
        &edit_button,
        "document-edit-symbolic",
        "编辑账户",
    )])));
    actions.append(&account_menu_button);

    summary_box.append(&status_line);
    summary_box.append(&status_detail);
    summary_box.append(&actions);
    page.append(&summary_box);

    let files_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();

    let files_scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .min_content_height(260)
        .build();
    files_scrolled.set_child(Some(&files_list));
    page.append(&files_scrolled);

    toolbar_view.set_content(Some(&page));
    (
        toolbar_view,
        ContentWidgets {
            title,
            status_title,
            status_detail,
            files_list,
            account_menu_button,
            settings_button,
            mode_dropdown,
            sync_button,
            preview_button,
            auth_button,
            edit_button,
        },
    )
}

pub(in crate::app) fn build_account_actions_popover(
    items: &[(&gtk::Button, &str, &str)],
) -> gtk::Popover {
    let popover = gtk::Popover::builder()
        .has_arrow(false)
        .position(gtk::PositionType::Bottom)
        .build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .width_request(ACCOUNT_CONTEXT_MENU_WIDTH)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build();

    for &(button, icon_name, label) in items {
        set_menu_button_content(button, icon_name, label);
        button.add_css_class("flat");
        button.set_halign(Align::Fill);
        content.append(button);
    }

    popover.set_child(Some(&content));
    popover
}

pub(in crate::app) fn build_profile_context_popover() -> ProfileContextPopover {
    let popover = gtk::Popover::builder()
        .has_arrow(false)
        .position(gtk::PositionType::Bottom)
        .build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build();

    let mut buttons = Vec::new();
    for (index, (icon_name, label)) in [
        ("view-refresh-symbolic", "同步一次"),
        ("media-playback-start-symbolic", "开始持续同步"),
        ("folder-open-symbolic", "打开同步目录"),
    ]
    .into_iter()
    .enumerate()
    {
        if index == 2 {
            content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        }
        let item = gtk::Button::builder()
            .halign(Align::Fill)
            .css_classes(["flat"])
            .build();
        set_menu_button_content(&item, icon_name, label);
        content.append(&item);
        buttons.push(item);
    }

    popover.set_child(Some(&content));
    ProfileContextPopover {
        popover,
        sync_once_button: buttons.remove(0),
        monitor_button: buttons.remove(0),
        open_sync_dir_button: buttons.remove(0),
    }
}

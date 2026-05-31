use super::state::AppState;
use adw::prelude::*;
use gtk::Align;
use std::rc::Rc;

pub(in crate::app) fn show_confirmation<F>(
    state: Rc<AppState>,
    title: &str,
    message: &str,
    confirm_label: &str,
    on_confirm: F,
) where
    F: Fn(Rc<AppState>) + 'static,
{
    let dialog = adw::Window::builder()
        .title(title)
        .transient_for(&state.window)
        .modal(true)
        .default_width(500)
        .build();
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    let header = adw::HeaderBar::new();
    let cancel_button = gtk::Button::with_label("取消");
    let confirm_button = gtk::Button::with_label(confirm_label);
    confirm_button.add_css_class("destructive-action");
    header.pack_start(&cancel_button);
    header.pack_end(&confirm_button);
    root.append(&header);
    let label = gtk::Label::builder()
        .label(message)
        .wrap(true)
        .halign(Align::Start)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();
    root.append(&label);

    let cancel_dialog = dialog.clone();
    cancel_button.connect_clicked(move |_| cancel_dialog.close());
    let confirm_dialog = dialog.clone();
    confirm_button.connect_clicked(move |_| {
        on_confirm(Rc::clone(&state));
        confirm_dialog.close();
    });
    dialog.set_content(Some(&root));
    dialog.present();
}

pub(in crate::app) fn show_warning_window(state: &AppState, title: &str, message: &str) {
    let dialog = adw::Window::builder()
        .title(title)
        .transient_for(&state.window)
        .modal(true)
        .default_width(560)
        .build();
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    let header = adw::HeaderBar::new();
    let close_button = gtk::Button::with_label("知道了");
    header.pack_end(&close_button);
    root.append(&header);
    let label = gtk::Label::builder()
        .label(message)
        .wrap(true)
        .halign(Align::Start)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();
    root.append(&label);
    let close_dialog = dialog.clone();
    close_button.connect_clicked(move |_| close_dialog.close());
    dialog.set_content(Some(&root));
    dialog.present();
}

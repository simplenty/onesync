use gtk::{Align, prelude::*};

pub(in crate::app) fn form_row(label: &str, entry: &gtk::Entry) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();
    let title = gtk::Label::builder()
        .label(label)
        .halign(Align::Start)
        .width_request(90)
        .build();
    row.append(&title);
    row.append(entry);
    entry.set_hexpand(true);
    row
}

pub(in crate::app) fn command_button(icon_name: &str, label: &str) -> gtk::Button {
    let button = gtk::Button::new();
    set_command_button_content(&button, icon_name, label);
    button
}

pub(in crate::app) fn set_command_button_content(button: &gtk::Button, icon_name: &str, label: &str) {
    let content = adw::ButtonContent::builder()
        .icon_name(icon_name)
        .label(label)
        .build();

    button.set_child(Some(&content));
}

pub(in crate::app) fn set_menu_button_content(button: &gtk::Button, icon_name: &str, label: &str) {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .halign(Align::Fill)
        .hexpand(true)
        .build();
    row.append(&gtk::Image::from_icon_name(icon_name));
    row.append(
        &gtk::Label::builder()
            .label(label)
            .halign(Align::Start)
            .hexpand(true)
            .build(),
    );
    button.set_child(Some(&row));
}

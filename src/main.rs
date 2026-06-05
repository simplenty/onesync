mod adapter;
mod app;
mod event;
mod operation;
mod profile;
mod utils;

fn main() -> gtk::glib::ExitCode {
    app::run()
}

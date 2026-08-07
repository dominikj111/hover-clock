mod backend;

use backend::WindowBackend;
use chrono::Local;
use gtk::{glib, prelude::*};

fn main() -> glib::ExitCode {
    let application = gtk::Application::builder()
        .application_id("com.github.gtk-rs.examples.clock")
        .build();
    application.connect_activate(build_ui);
    application.run()
}

fn build_ui(application: &gtk::Application) {
    let window = gtk::ApplicationWindow::new(application);

    window.set_title(Some("Clock Example"));
    window.set_default_size(260, 40);

    let time = current_time();
    let label = gtk::Label::default();
    label.set_text(&time);

    window.set_child(Some(&label));

    // M1: apply overlay semantics (EWMH hints, non-focusable window type).
    // Configured at realize time — before the window maps — so the window
    // manager reads the hints when it manages the window. Degrades to a
    // logged warning when X11 is unavailable.
    match backend::X11WindowBackend::new() {
        Ok(backend) => {
            window.connect_realize(move |window| backend.configure(window.upcast_ref()));
        }
        Err(err) => glib::g_warning!(
            "hover-clock",
            "X11 window backend unavailable; overlay behavior disabled: {err}"
        ),
    }

    // Show without present(): present() would ask the window manager for
    // focus, which the overlay must never do (proposal §6).
    window.set_visible(true);

    // we are using a closure to capture the label (else we could also use a normal
    // function)
    let tick = move || {
        let time = current_time();
        label.set_text(&time);
        // we could return glib::ControlFlow::Break to stop our clock after this tick
        glib::ControlFlow::Continue
    };

    // executes the closure once every second
    glib::timeout_add_seconds_local(1, tick);
}

fn current_time() -> String {
    format!("{}", Local::now().format("%Y-%m-%d %H:%M:%S"))
}

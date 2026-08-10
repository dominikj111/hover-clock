mod backend;

use std::cell::RefCell;
use std::rc::Rc;

use backend::{ActivationBackend, ActivationEvent, WindowBackend};
use chrono::Local;
use gtk::{glib, prelude::*};

/// Pointer must dwell in the hot corner this long before the overlay
/// shows (proposal §5: debounced; common value ~200 ms).
const CORNER_DWELL: std::time::Duration = std::time::Duration::from_millis(200);

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

    // M2: activation. The overlay starts hidden and surfaces on demand
    // (hot corner, Super+T); Esc dismisses. Backend failures degrade to
    // the M1 behavior: overlay always visible, warning logged.
    let window = Rc::new(window);
    let activation: Option<Rc<backend::X11ActivationBackend>> =
        match backend::X11ActivationBackend::new() {
            Ok(backend) => {
                let backend = Rc::new(backend);
                match backend.start() {
                    Ok(()) => Some(backend),
                    Err(err) => {
                        glib::g_warning!(
                            "hover-clock",
                            "activation unavailable, overlay stays visible: {err}"
                        );
                        None
                    }
                }
            }
            Err(err) => {
                glib::g_warning!("hover-clock", "X11 activation backend unavailable: {err}");
                None
            }
        };

    match &activation {
        Some(backend) => {
            let glue_window = Rc::clone(&window);
            let glue_backend = Rc::clone(backend);
            let dwell = Rc::new(RefCell::new(None::<glib::SourceId>));

            // Runs on the main context for every activation event.
            let dispatch = move |event: ActivationEvent| match event {
                ActivationEvent::CornerEntered { .. } => {
                    // Debounce: show only after the pointer dwells in the
                    // corner. Per-monitor placement is M3 (presentation).
                    if let Some(id) = dwell.borrow_mut().take() {
                        id.remove();
                    }
                    let window = Rc::clone(&glue_window);
                    let backend = Rc::clone(&glue_backend);
                    let dwell_timer = Rc::clone(&dwell);
                    let id = glib::timeout_add_local(CORNER_DWELL, move || {
                        show_overlay(&window, &backend);
                        *dwell_timer.borrow_mut() = None;
                        glib::ControlFlow::Break
                    });
                    *dwell.borrow_mut() = Some(id);
                }
                ActivationEvent::CornerLeft { .. } => {
                    // Auto-hide on pointer leave is M3; dismissal is Esc.
                    if let Some(id) = dwell.borrow_mut().take() {
                        id.remove();
                    }
                }
                ActivationEvent::Toggle => {
                    if let Some(id) = dwell.borrow_mut().take() {
                        id.remove();
                    }
                    if glue_window.is_visible() {
                        hide_overlay(&glue_window, &glue_backend);
                    } else {
                        show_overlay(&glue_window, &glue_backend);
                    }
                }
                ActivationEvent::Dismiss => {
                    if let Some(id) = dwell.borrow_mut().take() {
                        id.remove();
                    }
                    hide_overlay(&glue_window, &glue_backend);
                }
            };

            if let Err(err) = backend.install_event_source(Box::new(dispatch)) {
                glib::g_warning!("hover-clock", "activation event loop unavailable: {err}");
                window.set_visible(true);
            } else {
                // Hidden until a trigger fires.
                window.set_visible(false);
            }
        }
        None => window.set_visible(true),
    }

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

/// Show the overlay; the activation backend must know so it grabs Esc.
fn show_overlay(window: &gtk::ApplicationWindow, backend: &Rc<backend::X11ActivationBackend>) {
    window.set_visible(true);
    backend.set_overlay_visible(true);
}

/// Hide the overlay and release the dismissal grab.
fn hide_overlay(window: &gtk::ApplicationWindow, backend: &Rc<backend::X11ActivationBackend>) {
    window.set_visible(false);
    backend.set_overlay_visible(false);
}

fn current_time() -> String {
    format!("{}", Local::now().format("%Y-%m-%d %H:%M:%S"))
}

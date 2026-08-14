mod backend;

use std::cell::RefCell;
use std::rc::Rc;

use backend::{ActivationBackend, ActivationEvent, WindowBackend};
use chrono::Local;
use gtk::{glib, prelude::*};

/// Pointer must dwell in the hot corner this long before the overlay
/// shows (proposal §5: debounced; common value ~200 ms).
const CORNER_DWELL: std::time::Duration = std::time::Duration::from_millis(200);

/// Leave the hot corner this long before a visible overlay auto-hides
/// (proposal §5: auto-hide is debounced, not instant, to avoid flicker
/// when the pointer oscillates across the corner boundary). Slightly
/// longer than `CORNER_DWELL` so a re-entry cancels the hide in time.
const AUTO_HIDE_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

/// M3 — the clock widget (proposal §11): time, day, date labels in a
/// vertical stack, styled by the bundled stylesheet. Pure UI with no
/// system side effects; this struct is the widget boundary a future
/// widget registry / `WidgetProvider` (proposal §11) would swap out.
struct ClockWidget {
    time: gtk::Label,
    day: gtk::Label,
    date: gtk::Label,
}

impl ClockWidget {
    /// Build the widget tree: a `.clock-frame` (painted black margin
    /// strip, style.css) wrapping the `.clock-widget` box (1px white
    /// border, rounded, translucent black) holding the three labels, and
    /// fill them once.
    fn new() -> (gtk::Box, Self) {
        let time = gtk::Label::new(None);
        time.add_css_class("clock-time");
        let day = gtk::Label::new(None);
        day.add_css_class("clock-day");
        let date = gtk::Label::new(None);
        date.add_css_class("clock-date");

        for label in [&time, &day, &date] {
            label.set_halign(gtk::Align::Center);
        }

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("clock-widget");
        root.append(&time);
        root.append(&day);
        root.append(&date);

        // The frame paints the black margin strip; its 2px padding sits
        // between the window edge and the white border (which lives on
        // `.clock-widget`), so the border is visible on light desktops
        // too: black margin - white border - black widget background.
        let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
        frame.add_css_class("clock-frame");
        frame.append(&root);

        let widget = Self { time, day, date };
        widget.update();
        (frame, widget)
    }

    /// Refresh all labels from the current wall-clock time.
    fn update(&self) {
        let now = Local::now();
        self.time.set_text(&now.format("%H:%M:%S").to_string());
        self.day.set_text(&now.format("%A").to_string());
        self.date.set_text(&now.format("%-d %B %Y").to_string());
    }
}

/// Install the bundled stylesheet at APPLICATION priority so it overrides
/// the theme for this app only, never for other GTK programs.
fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(include_str!("style.css"));
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

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

    // M3: the overlay has no system frame. The NOTIFICATION window type
    // (M1) already suppresses it under xfwm4; `decorated(false)` makes
    // that independent of the window manager. A GTK window property, not
    // a system API — it stays in the UI layer (proposal §9).
    window.set_decorated(false);

    // M3: static single style — the bundled stylesheet (rounded corners,
    // translucent black, proposal §8.3). Future theming (S05 config)
    // selects between bundled stylesheets; style.css is the swap point.
    load_css();

    // M3: the clock widget (proposal §11) replaces the M0 single label.
    let (clock_root, clock) = ClockWidget::new();
    window.set_child(Some(&clock_root));

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
            let hide_timer = Rc::new(RefCell::new(None::<glib::SourceId>));

            // Runs on the main context for every activation event.
            let dispatch = move |event: ActivationEvent| match event {
                ActivationEvent::CornerEntered { .. } => {
                    // Back in the corner: cancel any pending auto-hide and
                    // (re)start the dwell debounce. Showing again is a
                    // no-op when the overlay is already visible. Per-monitor
                    // placement is M3 (presentation).
                    if let Some(id) = dwell.borrow_mut().take() {
                        id.remove();
                    }
                    if let Some(id) = hide_timer.borrow_mut().take() {
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
                    // Auto-hide: leaving the corner while the overlay is
                    // visible starts a debounced hide (proposal §5), not an
                    // instant dismissal — re-entering in time cancels it.
                    if let Some(id) = dwell.borrow_mut().take() {
                        id.remove();
                    }
                    if glue_window.is_visible() {
                        let window = Rc::clone(&glue_window);
                        let backend = Rc::clone(&glue_backend);
                        let hide_timer_cell = Rc::clone(&hide_timer);
                        let id = glib::timeout_add_local(AUTO_HIDE_DELAY, move || {
                            hide_overlay(&window, &backend);
                            *hide_timer_cell.borrow_mut() = None;
                            glib::ControlFlow::Break
                        });
                        *hide_timer.borrow_mut() = Some(id);
                    }
                }
                ActivationEvent::Toggle => {
                    if let Some(id) = dwell.borrow_mut().take() {
                        id.remove();
                    }
                    if let Some(id) = hide_timer.borrow_mut().take() {
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
                    if let Some(id) = hide_timer.borrow_mut().take() {
                        id.remove();
                    }
                    hide_overlay(&glue_window, &glue_backend);
                }
                ActivationEvent::WorkspaceChanged {
                    pointer_in_hot_area,
                } => {
                    if let Some(id) = dwell.borrow_mut().take() {
                        id.remove();
                    }
                    if let Some(id) = hide_timer.borrow_mut().take() {
                        id.remove();
                    }
                    if pointer_in_hot_area {
                        // Pointer is in the hot area on the new workspace:
                        // show immediately — the switch re-affirms the
                        // trigger, no dwell needed. Hide first so the
                        // window re-maps onto the current workspace (X11
                        // windows stay on the workspace they were mapped
                        // on); unmap+map in the same batch is
                        // imperceptible, so the overlay follows without
                        // flicker.
                        hide_overlay(&glue_window, &glue_backend);
                        show_overlay(&glue_window, &glue_backend);
                    } else {
                        // The overlay would otherwise linger on the
                        // workspace the user left.
                        hide_overlay(&glue_window, &glue_backend);
                    }
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

    // One update per second refreshes all three labels (day/date roll over
    // at midnight). The widget holds no timers of its own; a single
    // low-frequency source is the only work (proposal §13: no per-frame
    // allocations, no polling loops).
    let tick = move || {
        clock.update();
        glib::ControlFlow::Continue
    };
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

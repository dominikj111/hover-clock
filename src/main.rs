mod backend;
mod ipc;
mod update;
mod version;

use std::cell::RefCell;
use std::rc::Rc;

use backend::ActivationEvent;
use chrono::Local;
use clap::Parser;
use gtk::{glib, prelude::*};

/// HoverClock CLI — the primary surface (engineering guideline
/// "CLI-first & control plane"; proposal §7.1): one binary, two roles.
///
/// `hover-clock --start` (alias `-s`, and `--daemon` for compatibility)
/// starts the single-instance daemon. Any other invocation is a client:
/// it sends one command (default: `show`) to the running daemon over the
/// control socket. `--stop`/`--restart` (and the equivalent positional
/// `stop`/`restart` commands) manage the daemon process itself, so they
/// work with any init — no systemd unit required. `-h`/`--help` and
/// `-V`/`--version` always work.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {
    /// Start the daemon (single instance — a second start while one is
    /// live exits with an explanatory error).
    #[arg(short = 's', long, alias = "daemon")]
    start: bool,

    /// Stop the running daemon: it exits cleanly and releases the
    /// control socket.
    #[arg(long)]
    stop: bool,

    /// Restart the running daemon in place: it re-executes itself, so
    /// the process id stays the same and supervisors (systemd user
    /// unit, autostart) keep tracking it.
    #[arg(long)]
    restart: bool,

    /// Command sent to the daemon: show (default), hide, toggle,
    /// stop, restart.
    #[arg(value_name = "COMMAND")]
    command: Option<String>,
}

/// Pointer must dwell in the hot corner this long before the overlay
/// shows (proposal §5: debounced; common value ~200 ms).
const CORNER_DWELL: std::time::Duration = std::time::Duration::from_millis(200);

/// Leave the hot corner this long before a visible overlay auto-hides
/// (proposal §5: auto-hide is debounced, not instant, to avoid flicker
/// when the pointer oscillates across the corner boundary). Slightly
/// longer than `CORNER_DWELL` so a re-entry cancels the hide in time.
const AUTO_HIDE_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

/// M3 — fade in/out (proposal §5): a short opacity transition layered on
/// top of the instant show/hide — the window maps first, then fades, so
/// perceived latency stays under the <50 ms budget. 10 × 15 ms = 150 ms.
const FADE_STEPS: u32 = 10;
const FADE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(15);

/// M3 — placement: how far above the triggered monitor's vertical centre
/// the overlay appears (user preference; the strip trigger is full-width,
/// so a corner is arbitrary — the overlay reads best upper-middle).
const OVERLAY_UP_OFFSET: i32 = 100;

/// M3 — the clock widget (proposal §11): time, day, date labels in a
/// vertical stack, styled by the bundled stylesheet. Pure UI with no
/// system side effects; this struct is the widget boundary a future
/// widget registry / `WidgetProvider` (proposal §11) would swap out.
struct ClockWidget {
    /// `HH` and `MM` with the colon between them, all at the full clock
    /// size; the seconds (`SS`) render in a separate, smaller label
    /// right after, no separator colon.
    hours: gtk::Label,
    minutes: gtk::Label,
    seconds: gtk::Label,
    day: gtk::Label,
    date: gtk::Label,
    /// The plain label, or a click-to-update button when a newer release
    /// is available (S09) — which one is visible is driven by the GitHub
    /// release check via `VersionUi` (thread-local).
    version_label: gtk::Label,
    version_button: gtk::Button,
}

impl ClockWidget {
    /// Build the widget tree: a `.clock-frame` (painted black margin
    /// strip, style.css) wrapping the `.clock-widget` box (1px white
    /// border, rounded, translucent black) holding the three labels, and
    /// fill them once.
    fn new() -> (gtk::Box, Self) {
        let hours = gtk::Label::new(None);
        hours.add_css_class("clock-time");
        // The colon between hours and minutes is its own label so CSS
        // can add breathing room around it (style.css) — the digits stay
        // tight, the separator gets the space.
        let colon = gtk::Label::new(Some(":"));
        colon.add_css_class("clock-time");
        colon.add_css_class("clock-time-colon");
        let minutes = gtk::Label::new(None);
        minutes.add_css_class("clock-time");
        // Seconds: a separate, smaller label right after `HH:MM`, no
        // separator colon — the size difference is the visual cue. Pure
        // CSS (`.clock-seconds`), so a future theme swaps it wholesale.
        // Baseline-aligned so the seconds sit on the same line as the
        // minutes (the digits share one baseline, like HH:MM:SS did).
        let seconds = gtk::Label::new(None);
        seconds.add_css_class("clock-seconds");
        let time_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        time_row.append(&hours);
        time_row.append(&colon);
        time_row.append(&minutes);
        time_row.append(&seconds);
        hours.set_valign(gtk::Align::Baseline);
        colon.set_valign(gtk::Align::Baseline);
        minutes.set_valign(gtk::Align::Baseline);
        seconds.set_valign(gtk::Align::Baseline);
        let day = gtk::Label::new(None);
        day.add_css_class("clock-day");
        let date = gtk::Label::new(None);
        date.add_css_class("clock-date");
        let version_area = gtk::Box::new(gtk::Orientation::Vertical, 0);
        version_area.add_css_class("clock-version-area");
        // DEV badge: shown only when the running binary is a dev build
        // (debug profile — `cargo build`/`cargo run`), never on a
        // production release binary. Compile-time, so a dev build cannot
        // forget it and a prod binary cannot gain it.
        let version_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let dev_label = gtk::Label::new(Some("DEV"));
        dev_label.add_css_class("clock-version-dev");
        dev_label.set_visible(cfg!(debug_assertions));
        let version_label = gtk::Label::new(None);
        version_label.add_css_class("clock-version");
        // Shows the running version immediately; the GitHub release check
        // (version::latest_release) swaps in the button with the newer
        // version when one exists.
        version_label.set_text(&version::current_label());
        version_row.append(&dev_label);
        version_row.append(&version_label);
        let version_button = gtk::Button::new();
        version_button.add_css_class("clock-version-button");
        version_button.set_visible(false);
        version_area.append(&version_row);
        version_area.append(&version_button);

        for label in [&day, &date, &version_label] {
            label.set_halign(gtk::Align::Center);
        }
        // Center the time (HH:MM + seconds) and version rows as units.
        time_row.set_halign(gtk::Align::Center);
        // Center the DEV+version row as a unit (the row spans the widget
        // width otherwise, splitting the badge from the version text).
        version_row.set_halign(gtk::Align::Center);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("clock-widget");
        root.append(&time_row);
        root.append(&day);
        root.append(&date);
        root.append(&version_area);

        // The frame paints the black margin strip; its 2px padding sits
        // between the window edge and the white border (which lives on
        // `.clock-widget`), so the border is visible on light desktops
        // too: black margin - white border - black widget background.
        let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
        frame.add_css_class("clock-frame");
        frame.append(&root);

        let widget = Self {
            hours,
            minutes,
            seconds,
            day,
            date,
            version_label,
            version_button,
        };
        widget.update();
        (frame, widget)
    }

    /// Refresh all labels from the current wall-clock time.
    fn update(&self) {
        let now = Local::now();
        self.hours.set_text(&now.format("%H").to_string());
        self.minutes.set_text(&now.format("%M").to_string());
        self.seconds.set_text(&now.format("%S").to_string());
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
    let cli = Cli::parse();
    if cli.start {
        run_daemon()
    } else if cli.stop {
        run_client(Some("stop"))
    } else if cli.restart {
        run_client(Some("restart"))
    } else {
        run_client(cli.command.as_deref())
    }
}

/// Daemon mode: bind the control socket (single-instance guard) and run
/// the overlay service. The socket binds before GTK initializes, so a
/// second start fails fast with an explanatory error.
fn run_daemon() -> glib::ExitCode {
    let control_service = match ipc::bind_daemon() {
        Ok(service) => service,
        Err(message) => {
            eprintln!("hover-clock: {message}");
            return glib::ExitCode::FAILURE;
        }
    };
    println!(
        "hover-clock daemon running (control socket {})",
        ipc::socket_path().display()
    );

    let application = gtk::Application::builder()
        .application_id("com.github.gtk-rs.examples.clock")
        .build();
    application.connect_shutdown(|_| {
        // Best-effort socket cleanup on clean exit; a crashed daemon's
        // stale socket is reclaimed by the next bind (ipc::bind_daemon).
        let _ = std::fs::remove_file(ipc::socket_path());
    });
    application.connect_activate(move |application| build_ui(application, control_service.clone()));
    // The CLI is ours (clap, above); GTK must not re-parse it. Hand
    // `g_application_run` only the program name, or GApplication rejects
    // `--daemon` as an unknown option and exits.
    application.run_with_args(&["hover-clock"])
}

/// Client mode: send one command to the running daemon. No command
/// defaults to `show` — a manual run renders the overlay, same as a
/// hot-corner dwell (proposal §7.4).
fn run_client(command: Option<&str>) -> glib::ExitCode {
    let command = match command {
        Some(raw) => match ipc::Command::parse(raw) {
            Ok(command) => command,
            Err(message) => {
                eprintln!("hover-clock: {message}");
                eprintln!("hover-clock: known commands: show | hide | toggle | stop | restart");
                return glib::ExitCode::FAILURE;
            }
        },
        None => ipc::Command::Show,
    };
    match ipc::request(command) {
        Ok(response) => {
            println!("{response}");
            glib::ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("hover-clock: {message}");
            eprintln!("hover-clock: start the daemon first: `hover-clock --start`");
            glib::ExitCode::FAILURE
        }
    }
}

/// Re-exec the current binary as the daemon (`--start`), replacing this
/// process image in place. Returns `false` after warning when the exec
/// could not be performed (caller should stop instead). Never returns on
/// success — exec replaces the process, keeping the same PID so
/// supervisors (systemd user unit, autostart) keep tracking the daemon.
fn reexec_daemon() -> bool {
    use std::os::unix::process::CommandExt;
    // Resolve the executable path: exec'ing /proc/self/exe directly would
    // truncate the process name (comm) to "exe", breaking `pkill -x
    // hover-clock` in the lifecycle scripts and the systemd unit. The
    // resolved file name keeps comm = "hover-clock".
    let exe = match std::fs::read_link("/proc/self/exe") {
        Ok(path) => path,
        Err(err) => {
            glib::g_warning!(
                "hover-clock",
                "daemon restart: cannot resolve own path: {err}"
            );
            return false;
        }
    };
    let err = std::process::Command::new(exe)
        // Keep argv[0] clean for `ps` output (default would be the path).
        .arg0("hover-clock")
        .arg("--start")
        .exec();
    glib::g_warning!("hover-clock", "daemon restart failed: {err}");
    false
}

fn build_ui(application: &gtk::Application, control_service: gio::SocketService) {
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

    // M3: the widget's natural size, used to place the window before its
    // first map (after that, the window's allocated size is used).
    let (_, natural_width, _, _) = clock_root.measure(gtk::Orientation::Horizontal, -1);
    let (_, natural_height, _, _) = clock_root.measure(gtk::Orientation::Vertical, -1);
    let window_size = (natural_width, natural_height);

    // GitHub release check (proposal §11.2, S09): now, then hourly. The
    // HTTP runs on a worker thread; the label is updated on the main
    // loop — offline/failed checks leave it as-is.
    VERSION_UI.with(|ui| {
        *ui.borrow_mut() = Some(VersionUi::new(
            clock.version_label.clone(),
            clock.version_button.clone(),
        ));
    });
    run_version_check();
    let refresh = move || {
        run_version_check();
        glib::ControlFlow::Continue
    };
    glib::timeout_add_seconds_local(60 * 60, refresh);

    // Backend selection (proposal §10): native Wayland layer-shell when
    // the display supports it, X11 otherwise (also under XWayland, where
    // layer-shell is unavailable and the overlay keeps the §17.3 degraded
    // stacking). Absence degrades to a logged warning and a plain window,
    // never a crash.
    let (window_backend, activation): backend::Backends = backend::build_backends();

    // M1: apply overlay semantics before the window maps. The X11 hints
    // are written at realize time (EWMH — the surface must exist); the
    // layer-shell conversion must be requested before realize (the
    // library hooks the realize signal internally). `prepare` covers the
    // pre-realize step, `configure` the realize step — each backend
    // implements what its platform needs and ignores the other.
    if let Some(window_backend) = &window_backend {
        let realize_backend = Rc::clone(window_backend);
        window.connect_realize(move |window| realize_backend.configure(window.upcast_ref()));
        window_backend.prepare(window.upcast_ref());
    }

    // M3: realize once up front (GTK's own path — `gtk_widget_realize`,
    // not `gtk_native_realize` directly) so the X surface exists for
    // placement happens after the first map (the WM owns the position
    // before then). Realize ≠ map: the overlay
    // stays hidden until a trigger fires. This must run *after* the
    // realize handler above is connected: `gtk_widget_realize` emits the
    // realize signal synchronously and exactly once (GTK keeps toplevels
    // realized for their lifetime), so a handler connected after this
    // call would never fire — the overlay would map as a plain NORMAL
    // window (taskbar entry, focusable, not above fullscreen).
    gtk::prelude::WidgetExt::realize(&window);

    // M2: activation. The overlay starts hidden and surfaces on demand
    // (hot corner, Super+T); Esc dismisses. The factory constructed and
    // started the backend; failures there already degraded to the M1
    // behavior (overlay always visible, warning logged).
    let window = Rc::new(window);

    match &activation {
        Some(backend) => {
            let controller = OverlayController::new(
                Rc::clone(&window),
                Rc::clone(backend),
                window_backend,
                window_size,
            );
            let dwell = Rc::new(RefCell::new(None::<glib::SourceId>));
            let hide_timer = Rc::new(RefCell::new(None::<glib::SourceId>));
            let ipc_controller = Rc::clone(&controller);
            let ipc_dwell = Rc::clone(&dwell);
            let ipc_hide_timer = Rc::clone(&hide_timer);

            // Runs on the main context for every activation event.
            let dispatch = move |event: ActivationEvent| match event {
                ActivationEvent::CornerEntered { monitor } => {
                    // Back in the corner: cancel any pending auto-hide and
                    // (re)start the dwell debounce. Showing again is a
                    // no-op when the overlay is already visible.
                    if let Some(id) = dwell.borrow_mut().take() {
                        id.remove();
                    }
                    if let Some(id) = hide_timer.borrow_mut().take() {
                        id.remove();
                    }
                    // M3: placement — the overlay follows the monitor the
                    // pointer entered.
                    controller.set_monitor(monitor);
                    let controller = Rc::clone(&controller);
                    let dwell_timer = Rc::clone(&dwell);
                    let id = glib::timeout_add_local(CORNER_DWELL, move || {
                        controller.show();
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
                    if controller.is_visible() {
                        let controller = Rc::clone(&controller);
                        let hide_timer_cell = Rc::clone(&hide_timer);
                        let id = glib::timeout_add_local(AUTO_HIDE_DELAY, move || {
                            controller.hide();
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
                    controller.toggle();
                }
                ActivationEvent::Dismiss => {
                    if let Some(id) = dwell.borrow_mut().take() {
                        id.remove();
                    }
                    if let Some(id) = hide_timer.borrow_mut().take() {
                        id.remove();
                    }
                    controller.hide();
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
                        // the switch re-affirms the trigger. Hide first so
                        // the window re-maps onto the current workspace
                        // (X11 windows stay on the workspace they were
                        // mapped on), then re-show with a fade-in — the
                        // re-map stays instant (no gap), the clock fades
                        // back in instead of popping.
                        controller.hide_instant();
                        controller.show();
                    } else {
                        // The overlay would otherwise linger on the
                        // workspace the user left.
                        controller.hide_instant();
                    }
                }
            };

            if let Err(err) = backend.install_event_source(Box::new(dispatch)) {
                glib::g_warning!("hover-clock", "activation event loop unavailable: {err}");
                window.set_visible(true);
            } else {
                // Hidden until a trigger fires (daemon autostart).
                window.set_visible(false);
            }

            // Control plane (proposal §7.4): `hover-clock` and
            // `hover-clock show|hide|toggle|stop|restart` drive the
            // overlay and the daemon process over the socket with the
            // same semantics as the equivalent triggers. Served on the
            // main context via gio async (ipc::install) — the UI thread
            // never blocks on socket I/O.
            let app = application.clone();
            let ipc_dispatch = move |line: &str| -> String {
                match ipc::Command::parse(line) {
                    Ok(ipc::Command::Show) => {
                        cancel_overlay_timers(&ipc_dwell, &ipc_hide_timer);
                        ipc_controller.show();
                        "ok".into()
                    }
                    Ok(ipc::Command::Hide) => {
                        cancel_overlay_timers(&ipc_dwell, &ipc_hide_timer);
                        ipc_controller.hide();
                        "ok".into()
                    }
                    Ok(ipc::Command::Toggle) => {
                        cancel_overlay_timers(&ipc_dwell, &ipc_hide_timer);
                        ipc_controller.toggle();
                        "ok".into()
                    }
                    Ok(ipc::Command::Stop) => {
                        // Hide and quit. The quit is deferred a tick so
                        // the gio async task can write the response
                        // first; the shutdown handler removes the socket.
                        cancel_overlay_timers(&ipc_dwell, &ipc_hide_timer);
                        ipc_controller.hide_instant();
                        let app = app.clone();
                        glib::timeout_add_local_once(
                            std::time::Duration::from_millis(50),
                            move || app.quit(),
                        );
                        "ok".into()
                    }
                    Ok(ipc::Command::Restart) => {
                        // Re-exec in place after the response is written:
                        // same PID, so systemd/autostart keep tracking
                        // the daemon. The control socket fd closes on
                        // exec (CLOEXEC); the fresh daemon reclaims the
                        // stale socket file at bind.
                        cancel_overlay_timers(&ipc_dwell, &ipc_hide_timer);
                        ipc_controller.hide_instant();
                        let app = app.clone();
                        glib::timeout_add_local_once(
                            std::time::Duration::from_millis(50),
                            move || {
                                if !reexec_daemon() {
                                    app.quit();
                                }
                            },
                        );
                        "ok".into()
                    }
                    Err(err) => format!("error: {err}"),
                }
            };
            match ipc::install(control_service, ipc_dispatch) {
                Ok(control) => {
                    // Keep the control plane alive for the daemon's lifetime.
                    // SAFETY: the value is Send+Sync and only ever accessed on
                    // the main thread; set_data is unsafe only when aliased
                    // across threads, which cannot happen here.
                    unsafe { application.set_data("hover-clock-control-plane", control) };
                }
                Err(err) => {
                    glib::g_warning!("hover-clock", "control plane unavailable: {err}");
                }
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

// Main-thread handle to the version UI (label ↔ update button, S09).
// The release-check and update worker threads cannot touch GTK widgets
// (not Send), so results are dispatched back via `MainContext::invoke`
// and the widgets live here, on the main loop.
thread_local! {
    static VERSION_UI: RefCell<Option<VersionUi>> = const { RefCell::new(None) };
}

/// The version row under the clock: a plain label normally, a
/// click-to-update button when a newer release is available.
struct VersionUi {
    label: gtk::Label,
    button: gtk::Button,
    /// The pending release the button would install (set when orange).
    release: RefCell<Option<version::Release>>,
}

impl VersionUi {
    fn new(label: gtk::Label, button: gtk::Button) -> Self {
        let ui = Self {
            label,
            button,
            release: RefCell::new(None),
        };
        // The click handler reads the pending release from the
        // thread-local and runs the self-update on a worker thread.
        ui.button.connect_clicked(|button| {
            VERSION_UI.with(|ui| {
                let ui = ui.borrow();
                let Some(ui) = ui.as_ref() else {
                    return;
                };
                let Some(release) = ui.release.borrow().clone() else {
                    return;
                };
                button.set_label("updating…");
                button.set_sensitive(false);
                let current = version::running_version();
                std::thread::spawn(move || {
                    let result = update::run(&release);
                    glib::MainContext::default().invoke(move || {
                        VERSION_UI.with(|ui| {
                            let ui = ui.borrow();
                            let Some(ui) = ui.as_ref() else {
                                return;
                            };
                            match result {
                                // The daemon is restarting (systemd) or
                                // exiting (re-exec fallback); nothing to
                                // restore.
                                Ok(_) => {}
                                Err(err) => {
                                    glib::g_warning!("hover-clock", "auto-update failed: {err}");
                                    // Restore the orange button so the
                                    // click can be retried.
                                    let (text, _) =
                                        version::label_text(current, Some(release.version));
                                    ui.button.set_label(&text);
                                    ui.button.set_sensitive(true);
                                }
                            }
                        });
                    });
                });
            });
        });
        ui
    }

    /// Apply a check result: the plain label with the running version,
    /// or the update button when a newer release exists.
    fn show_version(&self, text: &str, outdated: bool, release: Option<version::Release>) {
        *self.release.borrow_mut() = release;
        if outdated {
            self.label.set_visible(false);
            self.button.set_visible(true);
            self.button.set_label(text);
        } else {
            self.button.set_visible(false);
            self.label.set_visible(true);
            self.label.set_text(text);
        }
    }
}

/// One version check: fetch the latest GitHub release on a worker thread
/// (blocking HTTP with timeouts) and apply the result to the version UI
/// on the main loop. Failed checks degrade to the current label — never
/// an error.
fn run_version_check() {
    let current = version::running_version();
    std::thread::spawn(move || {
        let latest = version::latest_release();
        let (text, outdated) = version::label_text(current, latest.as_ref().map(|r| r.version));
        // Send closure (text + flag + release); runs on the main loop,
        // where the non-Send widgets are reachable via the thread-local.
        glib::MainContext::default().invoke(move || {
            VERSION_UI.with(|ui| {
                let ui = ui.borrow();
                if let Some(ui) = ui.as_ref() {
                    ui.show_version(&text, outdated, latest);
                }
            });
        });
    });
}

/// Cancel any pending dwell/auto-hide timers — a trigger or control
/// command supersedes them.
fn cancel_overlay_timers(
    dwell: &Rc<RefCell<Option<glib::SourceId>>>,
    hide_timer: &Rc<RefCell<Option<glib::SourceId>>>,
) {
    if let Some(id) = dwell.borrow_mut().take() {
        id.remove();
    }
    if let Some(id) = hide_timer.borrow_mut().take() {
        id.remove();
    }
}

/// Owns the overlay window + activation backend and the show/hide
/// behaviour (M3): placement centred above the triggered monitor's
/// middle and the fade in/out transition (proposal §5/§8.3). The
/// dwell/auto-hide debounce timers stay in the event glue; this owns the
/// fade animation and the last-known hot-area monitor.
struct OverlayController {
    window: Rc<gtk::ApplicationWindow>,
    backend: Rc<dyn backend::ActivationBackend>,
    /// Window backend for placement; `None` when the platform backend is
    /// unavailable (placement degrades to GTK default).
    window_backend: Option<Rc<dyn backend::WindowBackend>>,
    /// Last monitor whose hot area was entered (`CornerEntered` carries
    /// the geometry; `Toggle` does not — it places on the last one).
    monitor: RefCell<Option<backend::Monitor>>,
    /// Measured natural widget size, used to place the window before its
    /// first map (after that, the window's allocated size is used).
    size: (i32, i32),
    /// The in-flight fade animation; replaced by any newer show/hide.
    fade: Rc<RefCell<Option<glib::SourceId>>>,
}

impl OverlayController {
    fn new(
        window: Rc<gtk::ApplicationWindow>,
        backend: Rc<dyn backend::ActivationBackend>,
        window_backend: Option<Rc<dyn backend::WindowBackend>>,
        size: (i32, i32),
    ) -> Rc<Self> {
        Rc::new(Self {
            window,
            backend,
            window_backend,
            monitor: RefCell::new(None),
            size,
            fade: Rc::new(RefCell::new(None)),
        })
    }

    fn is_visible(&self) -> bool {
        self.window.is_visible()
    }

    /// Remember which monitor's hot area was entered — the overlay is
    /// placed on it at the next show.
    fn set_monitor(&self, monitor: backend::Monitor) {
        *self.monitor.borrow_mut() = Some(monitor);
    }

    /// Position the window for the next show: horizontally centred on the
    /// last-known monitor (dwell trigger) — or the root screen before any
    /// dwell (manual `show`/`toggle`) — `OVERLAY_UP_OFFSET` above the
    /// vertical centre. Runs before the window maps; the backend marks the
    /// position program-specified (ICCCM USPosition) so the WM maps the
    /// window there directly — no render-then-jump. No-op until a backend
    /// is available; degrades to GTK default placement.
    fn place(&self) {
        let Some(window_backend) = &self.window_backend else {
            return;
        };
        let window: &gtk::Window = self.window.upcast_ref();
        let w = if self.window.width() > 0 {
            self.window.width()
        } else {
            self.size.0
        };
        let h = if self.window.height() > 0 {
            self.window.height()
        } else {
            self.size.1
        };
        let (cx, cy, cw, ch) = match *self.monitor.borrow() {
            Some(monitor) => (monitor.x, monitor.y, monitor.width, monitor.height),
            None => {
                let Some((sw, sh)) = window_backend.screen_size() else {
                    return;
                };
                (0, 0, sw, sh)
            }
        };
        window_backend.move_to(
            window,
            cx + (cw - w) / 2,
            cy + (ch - h) / 2 - OVERLAY_UP_OFFSET,
        );
    }

    /// Show with fade-in: the window maps at the placed position (latency
    /// budget, proposal §5), then fades in over ~150 ms.
    fn show(&self) {
        self.cancel_fade();
        self.place();
        self.window.set_opacity(0.0);
        self.window.set_visible(true);
        self.backend.set_overlay_visible(true);
        self.fade_to(1.0, |_, _| {});
    }

    /// Hide with fade-out: animate opacity to 0, then unmap and release
    /// the dismissal grab.
    fn hide(&self) {
        if !self.is_visible() {
            return;
        }
        self.fade_to(0.0, |window, backend| {
            window.set_visible(false);
            backend.set_overlay_visible(false);
        });
    }

    /// Toggle between show and hide (Super+T, IPC `toggle`).
    fn toggle(&self) {
        if self.is_visible() {
            self.hide();
        } else {
            self.show();
        }
    }

    fn hide_instant(&self) {
        self.cancel_fade();
        self.window.set_visible(false);
        self.backend.set_overlay_visible(false);
    }

    fn cancel_fade(&self) {
        if let Some(id) = self.fade.borrow_mut().take() {
            id.remove();
        }
    }

    /// Animate window opacity from its current value to `to` over
    /// `FADE_STEPS × FADE_INTERVAL`; `on_done` runs on the main loop when
    /// the animation completes. Replaces any in-flight fade.
    fn fade_to<F>(&self, to: f64, on_done: F)
    where
        F: Fn(&gtk::ApplicationWindow, &Rc<dyn backend::ActivationBackend>) + 'static,
    {
        self.cancel_fade();
        let start = self.window.opacity();
        let window = Rc::clone(&self.window);
        let backend = Rc::clone(&self.backend);
        let cell = Rc::clone(&self.fade);
        let mut step = 0;
        let id = glib::timeout_add_local(FADE_INTERVAL, move || {
            step += 1;
            let t = step as f64 / FADE_STEPS as f64;
            window.set_opacity(start + (to - start) * t);
            if step >= FADE_STEPS {
                on_done(&window, &backend);
                *cell.borrow_mut() = None;
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
        *self.fade.borrow_mut() = Some(id);
    }
}

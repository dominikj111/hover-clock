//! Platform backends behind trait contracts (proposal §10).
//!
//! X11 (M1–M2) and native Wayland layer-shell (M7) implementations sit
//! behind the same contracts; business logic never touches system APIs
//! directly — it goes through these facades.
//!
//! Policy note: the strict overlay contract — never takes focus, invisible
//! to task switchers, transparent/invisible activation regions — is
//! hover-clock use-case policy, NOT a property of these contracts. Future
//! operational shells and kiosk apps may relax it (fullscreen or windowed,
//! focusable, taskbar-visible) and activation may be an explicit
//! affordance (floating icon, kiosk button) instead of an invisible hot
//! area (proposal §16).

#[cfg(feature = "wayland")]
mod wayland;
mod x11;

#[cfg(feature = "wayland")]
pub use wayland::{WaylandActivationBackend, WaylandWindowBackend};
pub use x11::{X11ActivationBackend, X11WindowBackend};

use gtk::glib;
use std::rc::Rc;

/// The platform backend pair for the current session: window behavior +
/// input activation (proposal §10).
pub type Backends = (
    Option<Rc<dyn WindowBackend>>,
    Option<Rc<dyn ActivationBackend>>,
);

/// Construct the platform backends for the current session (proposal §10).
///
/// Native Wayland (layer-shell) wins when the display supports it;
/// X11 otherwise — including under XWayland, where layer-shell is
/// unavailable and the overlay keeps the §17.3 degraded stacking.
/// Missing platform support degrades to `None` with a logged warning
/// (the overlay then runs as a plain window), never a crash.
pub fn build_backends() -> Backends {
    #[cfg(feature = "wayland")]
    if gtk4_layer_shell::is_supported() {
        let activation = match WaylandActivationBackend::new().and_then(|backend| {
            backend.start()?;
            Ok(backend)
        }) {
            Ok(backend) => backend,
            Err(err) => {
                glib::g_warning!(
                    "hover-clock",
                    "Wayland activation backend unavailable; overlay stays hidden: {err}"
                );
                return (
                    Some(Rc::new(WaylandWindowBackend) as Rc<dyn WindowBackend>),
                    None,
                );
            }
        };
        return (
            Some(Rc::new(WaylandWindowBackend) as Rc<dyn WindowBackend>),
            Some(Rc::new(activation) as Rc<dyn ActivationBackend>),
        );
    }

    let window_backend = match X11WindowBackend::new() {
        Ok(backend) => Some(Rc::new(backend) as Rc<dyn WindowBackend>),
        Err(err) => {
            glib::g_warning!(
                "hover-clock",
                "X11 window backend unavailable; overlay behavior disabled: {err}"
            );
            None
        }
    };
    let activation = match X11ActivationBackend::new() {
        Ok(backend) => match backend.start() {
            Ok(()) => Some(Rc::new(backend) as Rc<dyn ActivationBackend>),
            Err(err) => {
                glib::g_warning!(
                    "hover-clock",
                    "activation unavailable, overlay stays visible: {err}"
                );
                None
            }
        },
        Err(err) => {
            glib::g_warning!("hover-clock", "X11 activation backend unavailable: {err}");
            None
        }
    };
    (window_backend, activation)
}

/// A monitor (output) in physical pixel coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Monitor {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Input activation events (proposal §5).
///
/// All triggers are edge-triggered: each transition is reported exactly
/// once. Debounce policy lives in the consumer (the show/hide glue), not
/// in the backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivationEvent {
    /// Pointer entered the hot-corner region of a monitor.
    CornerEntered { monitor: Monitor },
    /// Pointer left the hot-corner region of a monitor.
    CornerLeft { monitor: Monitor },
    /// Global shortcut pressed (`Super + T`).
    Toggle,
    /// Dismissal key pressed (`Esc`).
    Dismiss,
    /// The active workspace changed (EWMH `_NET_CURRENT_DESKTOP`).
    ///
    /// `pointer_in_hot_area` is re-evaluated on the new workspace (the
    /// pointer is shared across workspaces, so its position is
    /// authoritative). When true, the consumer shows the overlay
    /// immediately — the switch re-affirms the trigger, no dwell needed;
    /// the consumer hides first so the window re-maps onto the current
    /// workspace (X11 windows stay on the workspace they were mapped on).
    /// When false, the consumer hides the overlay, which would otherwise
    /// linger on the workspace the user left.
    WorkspaceChanged { pointer_in_hot_area: bool },
}

/// Contract for platform-specific input activation (proposal §10).
///
/// Implementations detect hot-corner entry/exit and global shortcuts,
/// edge-triggered. The overlay never takes focus (proposal §6), so
/// dismissal keys are grabbed only while the overlay is visible and
/// released when it hides. Missing platform support must degrade to a
/// logged warning, never a crash.
pub trait ActivationBackend {
    /// Acquire inputs: event selection, key grabs, monitor geometry.
    fn start(&self) -> Result<(), String>;

    /// Reflect overlay visibility so dismissal keys are only grabbed
    /// while the overlay is shown.
    fn set_overlay_visible(&self, visible: bool);

    /// Wire `dispatch` to the platform event pump. Called once, right
    /// after [`Self::start`]; every activation event is reported exactly
    /// once through `dispatch` (edge-triggered).
    fn install_event_source(
        &self,
        dispatch: Box<dyn Fn(ActivationEvent) + 'static>,
    ) -> Result<(), String>;
}

/// Contract for platform-specific overlay window behavior.
///
/// Implementations make a toplevel window behave as an overlay: stacked
/// above fullscreen apps, hidden from task switchers, never focusable
/// (proposal §9). Missing platform support must degrade to a logged
/// warning, never a crash.
pub trait WindowBackend: Send + Sync {
    /// Prepare the toplevel before it is realized. Platform setup that
    /// must precede surface creation goes here — the layer-shell backend
    /// turns the window into a layer surface at realize, so its setup
    /// hooks the realize signal internally and must run first. Default:
    /// nothing to do (X11 applies its hints at realize, in
    /// [`Self::configure`]).
    fn prepare(&self, _window: &gtk::Window) {}

    /// Apply overlay semantics to a toplevel window.
    ///
    /// Callers invoke this after the window has a surface (e.g. after
    /// `present()`); implementations skip silently when the platform
    /// cannot provide the behavior.
    fn configure(&self, window: &gtk::Window);

    /// Position the toplevel at absolute root coordinates (M3: the
    /// overlay appears centred above the triggered monitor's middle).
    /// Best-effort — missing platform support degrades to a no-op, never
    /// an error (proposal §10).
    fn move_to(&self, window: &gtk::Window, x: i32, y: i32) {
        let _ = (window, x, y);
    }

    /// Query the root screen geometry, for centring the overlay before a
    /// monitor is known. Best-effort — missing platform support degrades
    /// to `None`, never an error (proposal §10).
    fn screen_size(&self) -> Option<(i32, i32)> {
        None
    }
}

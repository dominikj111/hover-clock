//! Platform backends behind trait contracts (proposal §10).
//!
//! The X11 implementation is active for M1–M2; a Wayland layer-shell
//! backend lands behind the same contracts at M6. Business logic never
//! touches system APIs directly — it goes through these facades.

mod x11;

pub use x11::{X11ActivationBackend, X11WindowBackend};

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
}

/// Contract for platform-specific overlay window behavior.
///
/// Implementations make a toplevel window behave as an overlay: stacked
/// above fullscreen apps, hidden from task switchers, never focusable
/// (proposal §9). Missing platform support must degrade to a logged
/// warning, never a crash.
pub trait WindowBackend: Send + Sync {
    /// Apply overlay semantics to a toplevel window.
    ///
    /// Callers invoke this after the window has a surface (e.g. after
    /// `present()`); implementations skip silently when the platform
    /// cannot provide the behavior.
    fn configure(&self, window: &gtk::Window);
}

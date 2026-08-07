//! Platform backends behind trait contracts (proposal §10).
//!
//! The X11 implementation is active for M1; a Wayland layer-shell backend
//! lands behind the same contract at M6. Business logic never touches
//! system APIs directly — it goes through these facades.

mod x11;

pub use x11::X11WindowBackend;

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

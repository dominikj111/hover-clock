//! Wayland implementation of the backend contracts (proposal §9.2, §10).
//!
//! `WaylandWindowBackend` turns the overlay into a `zwlr_layer_shell_v1`
//! surface via gtk4-layer-shell: stacked above fullscreen apps (OVERLAY
//! layer, §15 decision), never focusable (keyboard mode NONE), placed by
//! layer-shell anchor + margins instead of X11 root coordinates.
//!
//! `WaylandActivationBackend` implements the hot corner with one
//! transparent 4 px top-strip layer surface per output. Wayland has no
//! global pointer-position API (proposal §15), so the strip *is* its own
//! input region: enter/leave crossing events fire when the pointer
//! reaches the monitor's top edge. Consequence, documented in §16: the
//! strip captures clicks in that 4 px band — the pointer never passes
//! through to the window below it.
//!
//! Global shortcuts (`Super + T`, `Esc`) have no portable protocol on
//! Wayland as of M7 (proposal §16 decision): `ext_global_shortcuts_v1` is
//! an unmerged upstream MR and the GlobalShortcuts portal has no wlroots
//! backend, so `set_overlay_visible` is a documented no-op here.

use std::cell::RefCell;
use std::rc::Rc;

use gio::prelude::ListModelExt;
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use super::{ActivationBackend, ActivationEvent, Monitor, WindowBackend};

/// Snapshot the display's monitors. GDK exposes them as a `gio::ListModel`
/// of `gdk::Monitor` objects (not a Rust iterator), so collect them here.
fn monitors() -> Vec<gdk::Monitor> {
    let Some(display) = gdk::Display::default() else {
        return Vec::new();
    };
    let model = display.monitors();
    let mut out = Vec::with_capacity(model.n_items() as usize);
    for i in 0..model.n_items() {
        let Some(object) = model.item(i) else {
            continue;
        };
        let Ok(monitor) = object.downcast::<gdk::Monitor>() else {
            continue;
        };
        out.push(monitor);
    }
    out
}

/// Hot-corner strip height in pixels — the Wayland corner is an input-
/// capturing band, so it needs to be hittable: on desktops with a top
/// bar the strip sits at the top of the *content* area (below the bar's
/// reserved band, wlroots free-area placement) and 4 px proved too thin
/// to aim at blind (handoff 07). 12 px is a compromise: reachable, yet
/// only the top 12 px of the content area is captured. The X11 corner
/// stays 4 px — it watches motion passively (monitor-absolute), so no
/// aiming precision is needed there.
const HOT_STRIP_HEIGHT: i32 = 12;

/// Wayland implementation of [`WindowBackend`].
///
/// Stateless: every method takes the window it acts on, and gtk4-layer-shell
/// keeps the per-window layer-shell state. The unit struct is the facade
/// marker — construction never fails (the factory checks
/// [`gtk4_layer_shell::is_supported`] first).
pub struct WaylandWindowBackend;

impl WindowBackend for WaylandWindowBackend {
    /// Turn the window into a layer surface. Must run before the window
    /// is realized: gtk4-layer-shell hooks the window's realize signal
    /// internally, and the hook can only attach while the window is
    /// still a plain toplevel.
    fn prepare(&self, window: &gtk::Window) {
        if window.is_layer_window() {
            return;
        }
        window.init_layer_shell();
        window.set_namespace(Some("hover-clock"));
        // OVERLAY is the topmost layer — designed for OSDs/notifications
        // (proposal §9.2, §15 "layer choice" decision). Above fullscreen
        // apps by construction; the X11 ABOVE hint is not needed.
        window.set_layer(Layer::Overlay);
        // Transient overlay: never reserves workspace (no exclusive
        // zone) and never takes focus or keyboard input.
        window.set_exclusive_zone(0);
        window.set_keyboard_mode(KeyboardMode::None);
        // Default placement: top-left of the monitor, no offset. The
        // controller re-positions via `move_to` before every show.
        window.set_anchor(Edge::Left, true);
        window.set_anchor(Edge::Top, true);
        window.set_anchor(Edge::Right, false);
        window.set_anchor(Edge::Bottom, false);
    }

    /// Layer-shell behaviour is applied at realize by the library's own
    /// hook (wired in [`Self::prepare`]); nothing to do post-map — the
    /// compositor owns stacking, so no re-application is ever needed.
    fn configure(&self, _window: &gtk::Window) {}

    /// Position the overlay at absolute root coordinates. Layer-shell has
    /// no absolute positioning: the surface lives on one output, so this
    /// switches the layer surface to the output containing `(x, y)` and
    /// expresses the offset as anchor margins relative to that output's
    /// origin. Best-effort: coordinates outside every output fall back to
    /// the primary output.
    fn move_to(&self, window: &gtk::Window, x: i32, y: i32) {
        let monitors = monitors();
        let output = monitors
            .iter()
            .find(|m| {
                let r = m.geometry();
                r.x() <= x && x < r.x() + r.width() && r.y() <= y && y < r.y() + r.height()
            })
            .or_else(|| monitors.first());
        let Some(output) = output else {
            return;
        };
        window.set_monitor(Some(output));
        let r = output.geometry();
        window.set_anchor(Edge::Left, true);
        window.set_anchor(Edge::Top, true);
        window.set_margin(Edge::Left, x - r.x());
        window.set_margin(Edge::Top, y - r.y());
    }

    /// Root-screen geometry for pre-trigger placement: the first output's
    /// logical size. GDK logical pixels throughout the Wayland path (the
    /// X11 backend reports physical pixels — the two never mix, see
    /// handoff 07). Wayland has no primary output, so the first is used.
    fn screen_size(&self) -> Option<(i32, i32)> {
        let r = monitors().first()?.geometry();
        Some((r.width(), r.height()))
    }
}

/// A transparent 4 px top-edge strip on one output: the Wayland hot
/// corner sensor. The surface's whole area is its input region, so it
/// receives pointer enter/leave — and swallows clicks in that band.
struct HotStrip {
    _window: gtk::Window,
}

/// The dispatch slot filled by [`ActivationBackend::install_event_source`];
/// strips route their crossing events through it. `None` only in the
/// window between `start()` and `install_event_source()`.
type DispatchSlot = Rc<RefCell<Option<Box<dyn Fn(ActivationEvent) + 'static>>>>;

/// Wayland implementation of [`ActivationBackend`].
pub struct WaylandActivationBackend {
    strips: RefCell<Vec<HotStrip>>,
    dispatch: DispatchSlot,
    /// Which monitor's strip currently holds the pointer (edge-trigger
    /// dedupe: GDK may deliver redundant enter/leave on surface switches).
    in_corner: Rc<RefCell<Option<Monitor>>>,
}

impl WaylandActivationBackend {
    /// Check the display exists and layer-shell is usable (the factory
    /// already verified `is_supported`; this re-checks cheaply so the
    /// backend never assumes).
    pub fn new() -> Result<Self, String> {
        if gdk::Display::default().is_none() {
            return Err("no GDK display".into());
        }
        Ok(Self {
            strips: RefCell::new(Vec::new()),
            dispatch: Rc::new(RefCell::new(None)),
            in_corner: Rc::new(RefCell::new(None)),
        })
    }

    /// Create one hot-corner strip per output and map it. The strips are
    /// persistent sensors — visible for the daemon's lifetime, unlike the
    /// transient overlay.
    fn build_strips(&self) -> Result<(), String> {
        let dispatch = Rc::clone(&self.dispatch);
        let in_corner = Rc::clone(&self.in_corner);

        for monitor in monitors() {
            let r = monitor.geometry();
            let strip = gtk::Window::new();
            strip.set_decorated(false);
            // Force the strip height: an empty window has no natural size,
            // and a 0 px surface would have no input region.
            let holder = gtk::Box::new(gtk::Orientation::Vertical, 0);
            holder.set_size_request(1, HOT_STRIP_HEIGHT);
            strip.set_child(Some(&holder));

            strip.init_layer_shell();
            strip.set_namespace(Some("hover-clock"));
            // OVERLAY layer. Placement note (proposal §16): wlroots places
            // a non-exclusive surface in the layer's *free* area, so the
            // strip sits at the top of the content area — below any
            // reserved chrome (e.g. the Pi OS PIXEL bar's 36 px band) —
            // never at the raw output edge. That is the correct Wayland
            // semantic: the corner triggers at the top of what the user
            // can interact with; overlaying the bar's band would steal its
            // input. The X11 corner keeps monitor-absolute semantics.
            strip.set_layer(Layer::Overlay);
            strip.set_anchor(Edge::Left, true);
            strip.set_anchor(Edge::Right, true);
            strip.set_anchor(Edge::Top, true);
            strip.set_exclusive_zone(0);
            strip.set_keyboard_mode(KeyboardMode::None);
            strip.set_monitor(Some(&monitor));

            let monitor_geom = Monitor {
                x: r.x(),
                y: r.y(),
                width: r.width(),
                height: r.height(),
            };
            let motion = gtk::EventControllerMotion::new();
            let dispatch_enter = Rc::clone(&dispatch);
            let in_corner_enter = Rc::clone(&in_corner);
            let strip_monitor = monitor_geom;
            motion.connect_enter(move |_, _, _| {
                if in_corner_enter.borrow().is_some() {
                    return;
                }
                *in_corner_enter.borrow_mut() = Some(strip_monitor);
                if let Some(dispatch) = dispatch_enter.borrow().as_ref() {
                    dispatch(ActivationEvent::CornerEntered {
                        monitor: strip_monitor,
                    });
                }
            });
            let dispatch_leave = Rc::clone(&dispatch);
            let in_corner_leave = Rc::clone(&in_corner);
            motion.connect_leave(move |_| {
                let Some(monitor) = in_corner_leave.borrow_mut().take() else {
                    return;
                };
                if let Some(dispatch) = dispatch_leave.borrow().as_ref() {
                    dispatch(ActivationEvent::CornerLeft { monitor });
                }
            });
            strip.add_controller(motion);

            strip.set_visible(true);
            self.strips.borrow_mut().push(HotStrip { _window: strip });
        }
        if self.strips.borrow().is_empty() {
            return Err("no outputs for hot-corner strips".into());
        }
        Ok(())
    }
}

impl ActivationBackend for WaylandActivationBackend {
    fn start(&self) -> Result<(), String> {
        // Strips need no sequencing — GDK delivers their crossing events
        // on the main loop once the windows are mapped.
        self.build_strips()?;
        glib::g_warning!(
            "hover-clock",
            "Wayland activation: no portable global-shortcut protocol on this compositor \
             (ext-global-shortcuts is unmerged upstream; the GlobalShortcuts portal has no \
             wlroots backend — proposal §16); Super+T/Esc unavailable, hot-corner only"
        );
        Ok(())
    }

    /// No dismissal-key grab exists on Wayland (see module docs): nothing
    /// to toggle. The overlay dismisses via corner-leave auto-hide and
    /// the IPC `hide` command.
    fn set_overlay_visible(&self, _visible: bool) {}

    fn install_event_source(
        &self,
        dispatch: Box<dyn Fn(ActivationEvent) + 'static>,
    ) -> Result<(), String> {
        *self.dispatch.borrow_mut() = Some(dispatch);
        Ok(())
    }
}

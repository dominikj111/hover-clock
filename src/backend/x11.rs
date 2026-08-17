//! X11 backends (proposal §9.1/§9.2).
//!
//! [`X11WindowBackend`]: overlay window hints (EWMH, ICCCM). Static hints
//! (`_NET_WM_WINDOW_TYPE`, `_NET_WM_STATE`) are written before the window
//! maps, so the window manager reads them at manage time. Once mapped,
//! hints are re-applied: GTK rewrites `WM_HINTS` when it shows the
//! surface, and the manager owns `_NET_WM_STATE` after manage (requested
//! via the EWMH client message). Any failure degrades to a logged warning
//! — overlay hints are best-effort, never fatal.
//!
//! **Taskbar flash (fixed):** GDK's show path (`set_initial_hints` in
//! `gdksurface-x11.c`) rebuilds `_NET_WM_STATE` from GDK's own toplevel
//! state immediately before mapping, and *deletes* the property when that
//! state is empty. A direct pre-map write therefore never reaches the WM's
//! manage read, so a tasklist (libwnck reads `_NET_WM_STATE_SKIP_TASKBAR`
//! at window-add; the NOTIFICATION type is *not* an exclusion) briefly
//! shows the overlay until the post-map EWMH state lands. Setting GDK's
//! X11 skip hints (`gdk_x11_surface_set_skip_*_hint`) makes
//! `set_initial_hints` write `_NET_WM_STATE_SKIP_TASKBAR`/`_SKIP_PAGER` on
//! GDK's own connection, in order, before the map request — the state is
//! present at MapNotify and the taskbar never lists the overlay.
//!
//! [`X11ActivationBackend`]: input activation (hot-corner, global
//! shortcuts). Pointer motion and key presses are watched on the root
//! window (event-driven, no polling); the hot corner is edge-triggered
//! per monitor. Workspace switches are detected via the EWMH
//! `_NET_CURRENT_DESKTOP` root property; the hot area is re-evaluated on
//! the new workspace, so the overlay follows the pointer into the corner
//! or hides when the pointer is elsewhere — it never lingers on the
//! workspace the user left. `Super + T` is grabbed for the daemon's
//! lifetime, `Esc` only while the overlay is visible (the overlay never
//! has focus, so dismissal cannot rely on window focus).

use std::cell::RefCell;
use std::os::fd::AsRawFd;
use std::rc::Rc;
use std::sync::Arc;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use x11rb::connection::Connection;
use x11rb::errors::{ConnectError, ReplyError};
use x11rb::protocol::Event;
use x11rb::protocol::randr::ConnectionExt as RandrConnectionExt;
use x11rb::protocol::xinput::{self, ConnectionExt as XinputConnectionExt, XIEventMask};
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ChangeWindowAttributesAux, ClientMessageData, ClientMessageEvent,
    ConnectionExt as XProtoConnectionExt, EventMask, GrabMode, KeyButMask, Keycode, ModMask,
    PropMode,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt;

use super::{ActivationBackend, ActivationEvent, Monitor, WindowBackend};

// GDK's X11 skip-hint setters (public header `gdkx11surface.h`, exported
// from `libgtk-4`; deprecated since 4.18 but present in 4.18/4.20).
//
// These are the *only* way to make GDK itself write `_NET_WM_STATE`
// instead of deleting it in its show path (see module docs — the taskbar
// flash). The flag lives on the toplevel; `set_initial_hints` reads it on
// every show, so one call at realize covers all show/hide cycles.
#[link(name = "gtk-4")]
unsafe extern "C" {
    fn gdk_x11_surface_set_skip_taskbar_hint(
        surface: *mut gdk::ffi::GdkSurface,
        skips_taskbar: glib::ffi::gboolean,
    );
    fn gdk_x11_surface_set_skip_pager_hint(
        surface: *mut gdk::ffi::GdkSurface,
        skips_pager: glib::ffi::gboolean,
    );
}

/// X11 implementation of [`WindowBackend`].
pub struct X11WindowBackend {
    conn: Arc<RustConnection>,
    root: u32,
}

impl X11WindowBackend {
    /// Connect to the default X11 display (`$DISPLAY`).
    pub fn new() -> Result<Self, ConnectError> {
        let (conn, screen) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen].root;
        Ok(Self {
            conn: Arc::new(conn),
            root,
        })
    }
}

impl WindowBackend for X11WindowBackend {
    fn configure(&self, window: &gtk::Window) {
        let Some(surface) = window.surface() else {
            glib::g_warning!(
                "hover-clock",
                "X11 backend: window has no surface yet; overlay hints skipped"
            );
            return;
        };
        let Some(x11_surface) = surface.downcast_ref::<gdk4_x11::X11Surface>() else {
            glib::g_warning!(
                "hover-clock",
                "X11 backend: window surface is not X11; overlay hints skipped"
            );
            return;
        };

        let xid = x11_surface.xid() as u32;

        // Tell GDK the overlay skips the taskbar/pager so its show path
        // writes `_NET_WM_STATE` (on GDK's own connection, before the map
        // request) instead of deleting it. Without this, the tasklist
        // briefly shows the overlay on every show until the post-map EWMH
        // state lands — the flash. Deprecated in GTK 4.18 but present; if
        // a future GTK removes the symbols the build fails loudly here.
        unsafe {
            let surface = x11_surface.as_ptr() as *mut gdk::ffi::GdkSurface;
            gdk_x11_surface_set_skip_taskbar_hint(surface, 1);
            gdk_x11_surface_set_skip_pager_hint(surface, 1);
        }

        let atoms = match self.intern_atoms() {
            Ok(atoms) => atoms,
            Err(err) => {
                glib::g_warning!(
                    "hover-clock",
                    "X11 backend: failed to intern EWMH atoms: {err}"
                );
                return;
            }
        };

        // Pre-map hints: the window manager reads these when it manages the
        // window. Configure runs at realize time, before the map request.
        if let Err(err) = self.write_static_hints(xid, &atoms) {
            glib::g_warning!(
                "hover-clock",
                "X11 backend: failed to apply overlay hints: {err}"
            );
        }

        // Post-map: GTK rewrites WM_HINTS when showing the surface, and the
        // manager takes ownership of _NET_WM_STATE once managed. Re-apply on
        // every map (also covers later show/hide cycles).
        let conn = Arc::clone(&self.conn);
        let root = self.root;
        surface.connect_mapped_notify(move |surface| {
            if !surface.is_mapped() {
                return;
            }
            let conn = Arc::clone(&conn);
            let atoms = atoms.clone();
            if let Err(err) = post_map_hints(&conn, root, xid, &atoms) {
                glib::g_warning!(
                    "hover-clock",
                    "X11 backend: failed to re-apply overlay hints: {err}"
                );
            }
            // The manager may still be finishing manage; retry once.
            glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
                if let Err(err) = post_map_hints(&conn, root, xid, &atoms) {
                    glib::g_warning!(
                        "hover-clock",
                        "X11 backend: failed to re-apply overlay hints: {err}"
                    );
                }
            });
        });
    }

    /// Position the toplevel at absolute root coordinates (M3: corner
    /// placement). Written on the backend's own connection before the
    /// window maps, so the manager maps it at the requested position.
    /// Best-effort: a missing surface or a non-X11 backend degrades to a
    /// no-op.
    fn move_to(&self, window: &gtk::Window, x: i32, y: i32) {
        let Some(surface) = window.surface() else {
            return;
        };
        let Some(x11_surface) = surface.downcast_ref::<gdk4_x11::X11Surface>() else {
            return;
        };
        let xid = x11_surface.xid() as u32;
        let _ = self.conn.configure_window(
            xid,
            &x11rb::protocol::xproto::ConfigureWindowAux::new().x(x).y(y),
        );
    }
}

impl X11WindowBackend {
    /// Hints the window manager reads when it manages the window.
    fn write_static_hints(&self, xid: u32, atoms: &Atoms) -> Result<(), ReplyError> {
        // _NET_WM_WINDOW_TYPE: NOTIFICATION — not focusable, not in alt-tab.
        self.conn
            .change_property32(
                PropMode::REPLACE,
                xid,
                atoms.net_wm_window_type,
                AtomEnum::ATOM,
                &[atoms.notification],
            )?
            .check()?;

        // _NET_WM_STATE: above fullscreen apps, hidden from taskbar/pager.
        self.conn
            .change_property32(
                PropMode::REPLACE,
                xid,
                atoms.net_wm_state,
                AtomEnum::ATOM,
                &[atoms.above, atoms.skip_taskbar, atoms.skip_pager],
            )?
            .check()?;

        write_wm_hints(&self.conn, xid)?;
        Ok(())
    }

    fn intern_atoms(&self) -> Result<Atoms, ReplyError> {
        Ok(Atoms {
            net_wm_state: self.intern_atom("_NET_WM_STATE")?,
            above: self.intern_atom("_NET_WM_STATE_ABOVE")?,
            skip_taskbar: self.intern_atom("_NET_WM_STATE_SKIP_TASKBAR")?,
            skip_pager: self.intern_atom("_NET_WM_STATE_SKIP_PAGER")?,
            net_wm_window_type: self.intern_atom("_NET_WM_WINDOW_TYPE")?,
            notification: self.intern_atom("_NET_WM_WINDOW_TYPE_NOTIFICATION")?,
        })
    }

    fn intern_atom(&self, name: &str) -> Result<Atom, ReplyError> {
        Ok(self.conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
    }
}

/// Re-apply hints after the window is mapped.
fn post_map_hints(
    conn: &RustConnection,
    root: u32,
    xid: u32,
    atoms: &Atoms,
) -> Result<(), ReplyError> {
    write_wm_hints(conn, xid)?;
    request_net_wm_state(conn, root, xid, atoms)?;
    Ok(())
}

/// ICCCM `WM_HINTS`: set input = false so the manager never gives the
/// overlay keyboard focus (proposal §9.1). Preserves the other hints that
/// GTK wrote (group leader etc.).
fn write_wm_hints(conn: &RustConnection, xid: u32) -> Result<(), ReplyError> {
    // Layout: [flags, input, initial_state, icon_pixmap, icon_window,
    //          icon_x, icon_y, icon_mask]; flag 0x1 = InputHint.
    let reply = conn
        .get_property(false, xid, AtomEnum::WM_HINTS, AtomEnum::WM_HINTS, 0, 9)?
        .reply()?;
    let mut hints = [0u32; 9];
    for (slot, chunk) in hints.iter_mut().zip(reply.value.chunks_exact(4)) {
        *slot = u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    hints[0] |= 0x1; // InputHint
    hints[1] = 0; // input = false
    conn.change_property32(
        PropMode::REPLACE,
        xid,
        AtomEnum::WM_HINTS,
        AtomEnum::WM_HINTS,
        &hints,
    )?
    .check()?;
    Ok(())
}

/// Request `_NET_WM_STATE_ABOVE`, `_NET_WM_STATE_SKIP_TASKBAR` and
/// `_NET_WM_STATE_SKIP_PAGER` via the EWMH client message (EWMH §1.5).
fn request_net_wm_state(
    conn: &RustConnection,
    root: u32,
    xid: u32,
    atoms: &Atoms,
) -> Result<(), ReplyError> {
    let mask = EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY;
    let request = |atom: Atom| -> Result<(), ReplyError> {
        let event = ClientMessageEvent::new(
            32,
            xid,
            atoms.net_wm_state,
            // [action=ADD, first atom, second atom, source=application, timestamp=CurrentTime]
            ClientMessageData::from([1, atoms.above, atom, 1, 0]),
        );
        conn.send_event(false, root, mask, event)?.check()?;
        Ok(())
    };
    request(atoms.skip_taskbar)?;
    request(atoms.skip_pager)?;
    Ok(())
}

/// Atoms used by the overlay backend.
#[derive(Clone)]
struct Atoms {
    net_wm_state: Atom,
    above: Atom,
    skip_taskbar: Atom,
    skip_pager: Atom,
    net_wm_window_type: Atom,
    notification: Atom,
}

// ---------------------------------------------------------------------------
// Activation backend (M2)
// ---------------------------------------------------------------------------

/// Keysym values (X11R7 keysymdef.h). Only the two we grab are listed.
mod keysym {
    pub const ESCAPE: u32 = 0xff1b; // XK_Escape
    pub const T: u32 = 0x0054; // XK_T
}

/// Size of the hot-area trigger region, in pixels. The default hot area
/// is the top-right corner of each monitor (proposal §5); location and
/// size become configurable in S05 — [`HotArea`] is the seam the config
/// value replaces.
const CORNER_SIZE: i32 = 4;

/// The hot-area trigger region, in screen coordinates.
///
/// The default is the top-right corner of a monitor. `x`/`y` are
/// absolute so the region generalizes to any location and size relative
/// to a monitor; S05 (config) will construct this from TOML.
#[derive(Clone, Copy, Debug)]
struct HotArea {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl HotArea {
    /// The top-right `size`×`size` region of `monitor`.
    fn top_right(monitor: Monitor, size: i32) -> Self {
        Self {
            x: monitor.x + monitor.width - size,
            y: monitor.y,
            width: size,
            height: size,
        }
    }

    /// True when the point lies inside the region.
    fn contains(self, x: i32, y: i32) -> bool {
        self.x <= x && x < self.x + self.width && self.y <= y && y < self.y + self.height
    }
}

/// X11 implementation of [`ActivationBackend`].
pub struct X11ActivationBackend {
    conn: Arc<RustConnection>,
    root: u32,
    state: Rc<RefCell<ActivationState>>,
}

/// Mutable detection state. Events are pumped by the glib source on the
/// main thread; all access stays on that thread, so `Rc<RefCell>` is safe.
#[derive(Default)]
struct ActivationState {
    monitors: Vec<Monitor>,
    toggle_keycode: Option<Keycode>,
    esc_keycode: Option<Keycode>,
    in_corner: Option<Monitor>,
    overlay_visible: bool,
    /// Last observed EWMH `_NET_CURRENT_DESKTOP` value; `None` while the
    /// WM does not advertise the property.
    last_desktop: Option<u32>,
    /// Interned `_NET_CURRENT_DESKTOP` atom, for property filtering.
    desktop_atom: Atom,
}

impl X11ActivationBackend {
    /// Connect to the default X11 display (`$DISPLAY`).
    pub fn new() -> Result<Self, ConnectError> {
        let (conn, screen) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen].root;
        Ok(Self {
            conn: Arc::new(conn),
            root,
            state: Rc::new(RefCell::new(ActivationState::default())),
        })
    }

    /// Install a glib source that drains X events on the main context and
    /// dispatches them to `dispatch`. The source watches a dup of the X
    /// connection fd, so events arrive without polling.
    pub fn install_event_source(
        &self,
        dispatch: Box<dyn Fn(ActivationEvent) + 'static>,
    ) -> Result<glib::SourceId, String> {
        // gio takes ownership of its fd; hand it a dup so x11rb keeps its own.
        let fd = self.conn.stream().as_raw_fd();
        let dup_fd = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) }
            .try_clone_to_owned()
            .map_err(|err| format!("dup() of X11 connection fd failed: {err}"))?;
        let socket = gio::Socket::from_fd(dup_fd)
            .map_err(|err| format!("gio socket for X11 connection failed: {err}"))?;

        let conn = Arc::clone(&self.conn);
        let root = self.root;
        let state = Rc::clone(&self.state);
        let source = gio::prelude::SocketExtManual::create_source(
            &socket,
            glib::IOCondition::IN,
            None::<&gio::Cancellable>,
            Some("hover-clock-x11-activation"),
            glib::Priority::DEFAULT,
            move |_, _| {
                for event in poll_events(&conn, root, &state) {
                    dispatch(event);
                }
                glib::ControlFlow::Continue
            },
        );
        Ok(source.attach(None))
    }
}

impl ActivationBackend for X11ActivationBackend {
    fn start(&self) -> Result<(), String> {
        // Watch pointer motion via XI2. XISelectEvents is a per-client
        // selection that does not touch the core root event mask, which
        // the window manager owns (SUBSTRUCTURE_REDIRECT). The hot corner
        // is therefore event-driven, no polling (proposal §13). Keyboard
        // arrives via the core passive grabs below; grabbed key events are
        // delivered without any event selection.
        let _ = self
            .conn
            .xinput_xi_query_version(2, 0)
            .ok()
            .and_then(|cookie| cookie.reply().ok());
        self.conn
            .xinput_xi_select_events(
                self.root,
                &[xinput::EventMask {
                    deviceid: 0, // XIAllDevices
                    mask: vec![XIEventMask::MOTION, XIEventMask::from(0u32)],
                }],
            )
            .map_err(|err| err.to_string())?
            .check()
            .map_err(|err| err.to_string())?;

        let monitors = query_monitors(&self.conn, self.root)?;
        let (toggle_keycode, esc_keycode) = lookup_keycodes(&self.conn)?;

        // Workspace tracking: watch the EWMH `_NET_CURRENT_DESKTOP` root
        // property. The overlay window stays on the workspace it was
        // mapped on; when the user switches away, the consumer hides it,
        // so the overlay never lingers on a workspace the user has left
        // and the next trigger shows it on the current one. Degrades to a
        // warning when the WM does not advertise the property.
        let desktop_atom = self
            .conn
            .intern_atom(false, b"_NET_CURRENT_DESKTOP")
            .map_err(|err| err.to_string())?
            .reply()
            .map_err(|err| err.to_string())?
            .atom;
        let current_desktop = read_current_desktop(&self.conn, self.root, desktop_atom);
        if current_desktop.is_none() {
            glib::g_warning!(
                "hover-clock",
                "X11 activation: WM does not advertise _NET_CURRENT_DESKTOP; \
                 workspace-change hiding disabled"
            );
        }
        self.conn
            .change_window_attributes(
                self.root,
                &ChangeWindowAttributesAux {
                    event_mask: Some(EventMask::PROPERTY_CHANGE),
                    ..Default::default()
                },
            )
            .map_err(|err| err.to_string())?
            .check()
            .map_err(|err| err.to_string())?;

        // Global shortcut: grab `Super + T` for the daemon's lifetime.
        // Grab the four lock-state combinations so NumLock/CapsLock do not
        // silently disable the shortcut (standard grabber practice).
        for modifiers in lock_state_combos() {
            let mask = ModMask::from(u16::from(ModMask::M4) | u16::from(modifiers));
            let Some(keycode) = toggle_keycode else {
                continue;
            };
            match self.conn.grab_key(
                false,
                self.root,
                mask,
                keycode,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
            ) {
                Ok(cookie) => {
                    if let Err(err) = cookie.check() {
                        glib::g_warning!(
                            "hover-clock",
                            "X11 activation: Super+T grab failed (already taken?): {err}"
                        );
                    }
                }
                Err(err) => glib::g_warning!(
                    "hover-clock",
                    "X11 activation: Super+T grab failed (already taken?): {err}"
                ),
            }
        }

        // `Esc` is grabbed only while the overlay is visible (see
        // set_overlay_visible) so the key stays free for other apps.
        *self.state.borrow_mut() = ActivationState {
            monitors,
            toggle_keycode,
            esc_keycode,
            last_desktop: current_desktop,
            desktop_atom,
            ..ActivationState::default()
        };

        self.conn.flush().map_err(|err| err.to_string())?;
        Ok(())
    }

    fn set_overlay_visible(&self, visible: bool) {
        let mut state = self.state.borrow_mut();
        if state.overlay_visible == visible {
            return;
        }
        state.overlay_visible = visible;
        let Some(esc) = state.esc_keycode else {
            return;
        };
        drop(state);
        for modifiers in lock_state_combos() {
            if visible {
                match self.conn.grab_key(
                    false,
                    self.root,
                    modifiers,
                    esc,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                ) {
                    Ok(cookie) => {
                        if let Err(err) = cookie.check() {
                            glib::g_warning!(
                                "hover-clock",
                                "X11 activation: Esc grab failed: {err}"
                            );
                        }
                    }
                    Err(err) => {
                        glib::g_warning!("hover-clock", "X11 activation: Esc grab failed: {err}")
                    }
                }
            } else if let Err(err) = self.conn.ungrab_key(esc, self.root, modifiers) {
                glib::g_warning!("hover-clock", "X11 activation: Esc ungrab failed: {err}");
            }
        }
        let _ = self.conn.flush();
    }
}

/// Drain pending X events into activation events (non-blocking).
fn poll_events(
    conn: &RustConnection,
    root: u32,
    state: &Rc<RefCell<ActivationState>>,
) -> Vec<ActivationEvent> {
    let mut out = Vec::new();
    loop {
        match conn.poll_for_event() {
            Ok(Some(event)) => handle_event(conn, root, state, &event, &mut out),
            Ok(None) => break,
            Err(err) => {
                glib::g_warning!("hover-clock", "X11 activation: connection error: {err}");
                break;
            }
        }
    }
    out
}

fn handle_event(
    conn: &RustConnection,
    root: u32,
    state: &Rc<RefCell<ActivationState>>,
    event: &Event,
    out: &mut Vec<ActivationEvent>,
) {
    match event {
        // XI2 motion: root coordinates are 16.16 fixed-point.
        Event::XinputMotion(event) => {
            let mut state = state.borrow_mut();
            let x = (event.root_x as f64 / 65536.0) as i32;
            let y = (event.root_y as f64 / 65536.0) as i32;
            let corner = state
                .monitors
                .iter()
                .copied()
                .find(|m| HotArea::top_right(*m, CORNER_SIZE).contains(x, y));
            match (state.in_corner.take(), corner) {
                (None, Some(monitor)) => {
                    state.in_corner = Some(monitor);
                    out.push(ActivationEvent::CornerEntered { monitor });
                }
                (Some(monitor), None) => {
                    out.push(ActivationEvent::CornerLeft { monitor });
                }
                (_, corner) => state.in_corner = corner,
            }
        }
        Event::KeyPress(event) => {
            let state = state.borrow();
            if Some(event.detail) == state.toggle_keycode
                && u16::from(event.state) & u16::from(KeyButMask::MOD4) != 0
            {
                out.push(ActivationEvent::Toggle);
            } else if Some(event.detail) == state.esc_keycode {
                out.push(ActivationEvent::Dismiss);
            }
        }
        // EWMH workspace switch: the WM updates `_NET_CURRENT_DESKTOP`
        // on the root whenever the active workspace changes.
        Event::PropertyNotify(event) => {
            if event.atom != state.borrow().desktop_atom {
                return;
            }
            // Re-read the value — the WM may have just created the
            // property or updated it. Emit only on an actual change, so
            // repeated notifications do not spam hide/show.
            let Some(desktop) = read_current_desktop(conn, root, state.borrow().desktop_atom)
            else {
                return;
            };
            if Some(desktop) == state.borrow().last_desktop {
                return;
            }

            // Workspace switched. Re-evaluate the hot area on the new
            // workspace with the authoritative pointer position (the
            // pointer is shared across workspaces, and the WM may warp it
            // on switch). Pointer in the hot area → the overlay follows;
            // elsewhere → it hides and must be re-triggered.
            let pointer = query_pointer(conn, root);
            let mut state = state.borrow_mut();
            state.last_desktop = Some(desktop);
            let corner = pointer.and_then(|(x, y)| {
                state
                    .monitors
                    .iter()
                    .copied()
                    .find(|m| HotArea::top_right(*m, CORNER_SIZE).contains(x, y))
            });
            match corner {
                Some(monitor) => {
                    state.in_corner = Some(monitor);
                    out.push(ActivationEvent::WorkspaceChanged {
                        pointer_in_hot_area: true,
                    });
                }
                None => {
                    state.in_corner = None;
                    out.push(ActivationEvent::WorkspaceChanged {
                        pointer_in_hot_area: false,
                    });
                }
            }
        }
        _ => {}
    }
}

/// Query the current core-pointer position on the root.
fn query_pointer(conn: &RustConnection, root: u32) -> Option<(i32, i32)> {
    let reply = conn.query_pointer(root).ok()?.reply().ok()?;
    Some((reply.root_x as i32, reply.root_y as i32))
}

/// Read the current workspace from the EWMH `_NET_CURRENT_DESKTOP`
/// property (CARDINAL 32 on the root); `None` when the WM does not
/// advertise it.
fn read_current_desktop(conn: &RustConnection, root: u32, atom: Atom) -> Option<u32> {
    conn.get_property(false, root, atom, AtomEnum::CARDINAL, 0, 1)
        .ok()?
        .reply()
        .ok()?
        .value32()?
        .next()
}

/// Monitor geometry via RandR 1.5; falls back to the root geometry when
/// the extension is unavailable.
fn query_monitors(conn: &RustConnection, root: u32) -> Result<Vec<Monitor>, String> {
    match conn
        .randr_get_monitors(root, true)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
    {
        Some(reply) if !reply.monitors.is_empty() => Ok(reply
            .monitors
            .iter()
            .map(|m| Monitor {
                x: m.x as i32,
                y: m.y as i32,
                width: m.width as i32,
                height: m.height as i32,
            })
            .collect()),
        Some(_) | None => {
            let screen = &conn.setup().roots[0];
            Ok(vec![Monitor {
                x: 0,
                y: 0,
                width: screen.width_in_pixels as i32,
                height: screen.height_in_pixels as i32,
            }])
        }
    }
}

/// Map the keysyms we grab to keycodes via the keyboard mapping, so the
/// grabs work independently of the active layout.
fn lookup_keycodes(conn: &RustConnection) -> Result<(Option<Keycode>, Option<Keycode>), String> {
    let setup = conn.setup();
    let min = setup.min_keycode;
    let count = setup.max_keycode - min + 1;
    let reply = conn
        .get_keyboard_mapping(min, count)
        .map_err(|err| err.to_string())?
        .reply()
        .map_err(|err| err.to_string())?;
    let per_keycode = reply.keysyms_per_keycode as usize;

    // First keycode whose keysym list contains the target. The T key lists
    // both XK_t and XK_T (unshifted/shifted columns).
    let find = |keysym: u32| {
        reply
            .keysyms
            .iter()
            .position(|sym| *sym == keysym)
            .map(|index| min + (index / per_keycode) as u8)
    };
    let toggle = find(keysym::T).or_else(|| find(u32::from(b't')));
    let esc = find(keysym::ESCAPE);
    Ok((toggle, esc))
}

/// The four lock-state modifier combinations to grab per key, so
/// NumLock/CapsLock state does not silently disable the shortcut.
fn lock_state_combos() -> [ModMask; 4] {
    let lock = ModMask::LOCK;
    let num = ModMask::M2;
    [
        ModMask::default(),
        lock,
        num,
        ModMask::from(u16::from(lock) | u16::from(num)),
    ]
}

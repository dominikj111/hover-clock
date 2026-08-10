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
//! [`X11ActivationBackend`]: input activation (hot-corner, global
//! shortcuts). Pointer motion and key presses are watched on the root
//! window (event-driven, no polling); the hot corner is edge-triggered
//! per monitor. `Super + T` is grabbed for the daemon's lifetime, `Esc`
//! only while the overlay is visible (the overlay never has focus, so
//! dismissal cannot rely on window focus).

use std::cell::RefCell;
use std::os::fd::AsRawFd;
use std::rc::Rc;
use std::sync::Arc;

use gtk::glib;
use gtk::prelude::*;
use x11rb::connection::Connection;
use x11rb::errors::{ConnectError, ReplyError};
use x11rb::protocol::randr::ConnectionExt as RandrConnectionExt;
use x11rb::protocol::xinput::{self, ConnectionExt as XinputConnectionExt, XIEventMask};
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ClientMessageData, ClientMessageEvent, ConnectionExt as XProtoConnectionExt,
    EventMask, GrabMode, KeyButMask, Keycode, ModMask, PropMode,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt;

use super::{ActivationBackend, ActivationEvent, Monitor, WindowBackend};

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
        let atoms = match self.intern_atoms() {
            Ok(atoms) => atoms,
            Err(err) => {
                glib::g_warning!("hover-clock", "X11 backend: failed to intern EWMH atoms: {err}");
                return;
            }
        };

        // Pre-map hints: the window manager reads these when it manages the
        // window. Configure runs at realize time, before the map request.
        if let Err(err) = self.write_static_hints(xid, &atoms) {
            glib::g_warning!("hover-clock", "X11 backend: failed to apply overlay hints: {err}");
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

/// Size of the hot-corner trigger region, in pixels. The trigger is the
/// top-right corner of each monitor (proposal §5).
const CORNER_SIZE: i32 = 4;

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
        let state = Rc::clone(&self.state);
        let source =
            gio::prelude::SocketExtManual::create_source(&socket, glib::IOCondition::IN, None::<&gio::Cancellable>, Some("hover-clock-x11-activation"), glib::Priority::DEFAULT, move |_, _| {
                for event in poll_events(&conn, &state) {
                    dispatch(event);
                }
                glib::ControlFlow::Continue
            });
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

        // Global shortcut: grab `Super + T` for the daemon's lifetime.
        // Grab the four lock-state combinations so NumLock/CapsLock do not
        // silently disable the shortcut (standard grabber practice).
        for modifiers in lock_state_combos() {
            let mask = ModMask::from(u16::from(ModMask::M4) | u16::from(modifiers));
            let Some(keycode) = toggle_keycode else {
                continue;
            };
            match self
                .conn
                .grab_key(false, self.root, mask, keycode, GrabMode::ASYNC, GrabMode::ASYNC)
            {
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
                match self
                    .conn
                    .grab_key(false, self.root, modifiers, esc, GrabMode::ASYNC, GrabMode::ASYNC)
                {
                    Ok(cookie) => {
                        if let Err(err) = cookie.check() {
                            glib::g_warning!("hover-clock", "X11 activation: Esc grab failed: {err}");
                        }
                    }
                    Err(err) => glib::g_warning!("hover-clock", "X11 activation: Esc grab failed: {err}"),
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
    state: &Rc<RefCell<ActivationState>>,
) -> Vec<ActivationEvent> {
    let mut out = Vec::new();
    loop {
        match conn.poll_for_event() {
            Ok(Some(event)) => handle_event(state, &event, &mut out),
            Ok(None) => break,
            Err(err) => {
                glib::g_warning!("hover-clock", "X11 activation: connection error: {err}");
                break;
            }
        }
    }
    out
}

fn handle_event(state: &Rc<RefCell<ActivationState>>, event: &Event, out: &mut Vec<ActivationEvent>) {
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
                .find(|m| m.x <= x && x < m.x + m.width && m.y <= y && y < m.y + m.height)
                .filter(|m| in_top_right_corner(*m, x, y));
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
        _ => {}
    }
}

/// True when `(x, y)` lies inside the top-right corner region of `monitor`.
fn in_top_right_corner(monitor: Monitor, x: i32, y: i32) -> bool {
    x >= monitor.x + monitor.width - CORNER_SIZE && y <= monitor.y + CORNER_SIZE
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

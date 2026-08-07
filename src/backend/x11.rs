//! X11 overlay window backend (proposal §9.1).
//!
//! Static hints (`_NET_WM_WINDOW_TYPE`, `_NET_WM_STATE`) are written
//! before the window maps, so the window manager reads them at manage
//! time. Once mapped, hints are re-applied: GTK rewrites `WM_HINTS` when
//! it shows the surface, and the manager owns `_NET_WM_STATE` after
//! manage (requested via the EWMH client message). Any failure degrades
//! to a logged warning — overlay hints are best-effort, never fatal.

use std::sync::Arc;

use gtk::glib;
use gtk::prelude::*;
use x11rb::connection::Connection;
use x11rb::errors::{ConnectError, ReplyError};
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ClientMessageData, ClientMessageEvent, ConnectionExt as XProtoConnectionExt,
    EventMask, PropMode,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt;

use super::WindowBackend;

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

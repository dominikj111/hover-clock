# Hand-off — Autostart fix (daemon never starts after reboot)

- **Type:** packaging bugfix (outside story cards; install/daemon layer from the packaging commits)
- **Status:** ✅ 2026-08-16 (fix applied + validated on the MX Linux/xfce test machine)
- **Next story:** S04 M3 Presentation (unchanged)

## Bug

After `scripts/install.sh` the overlay works immediately, but after a machine reboot it does
not — the daemon never runs. Confirmed on the MX Linux/xfce (xfwm4) test machine.

## Root cause

The systemd *user* unit was `WantedBy=graphical-session.target` **only**. On this system the
xfce session **never raises** `graphical-session.target` — the user-session journal shows the
manager reaching `default.target` straight away (xfce4-session has no systemd
graphical-session integration; only GNOME/KDE raise it). The unit therefore never auto-starts
at login. `install.sh` works in the moment because `systemctl --user enable --now` starts it
immediately; the wants-link only fires when its target is reached, which never happens here.

Diagnostic that pinned it: `journalctl --user -u graphical-session.target` → no entries
across any boot, while `systemctl --user list-units --type=target | grep graphical` was empty.

## Fix

`packaging/hover-clock.service` is now wanted by **both** targets:

```
[Unit]
After=default.target graphical-session.target
PartOf=graphical-session.target
[Install]
WantedBy=default.target graphical-session.target
```

`default.target` is reached by every systemd user session (including xfce on MX), so the
daemon starts at login there; on GNOME/KDE the later `graphical-session.target` finds it
already active — exactly one start either way. `PartOf=graphical-session.target` is kept so
GNOME/KDE logout still stops it with the session.

The user-manager environment already carries `DISPLAY=:0`/`XAUTHORITY` at login (lightdm sets
them via pam_systemd before the user manager starts — verified with
`systemctl --user show-environment`), so the daemon connects at `default.target` without
hardcoding a display. The unit comment still documents the explicit
`Environment=DISPLAY=:0 XAUTHORITY=%h/.Xauthority` fallback for console-login + manual startx.

## Validation

- `systemd-analyze verify packaging/hover-clock.service` — clean.
- `systemctl --user enable` created BOTH wants links:
  `~/.config/systemd/user/default.target.wants/` and `.../graphical-session.target.wants/`.
- `systemctl --user list-dependencies default.target | grep hover-clock` — 1 hit.
- Live daemon active under systemd (`systemctl --user start` → `active`, stable).

## When targeting KDE/GNOME (verify list, recorded so it is not lost)

The whole autostart story differs by DE at login; this is the durable summary for the day
KDE/GNOME testing starts (relevant to S08/M7 and to §17.2):

- **GNOME/KDE raise `graphical-session.target`** (their session managers do the systemd
  integration) — on those DEs the *original* single-target unit would have auto-started
  correctly. The bug was xfce-specific (never raises it).
- **The dual-target unit is correct on all three**: `default.target` is reached at login on
  every systemd user session → daemon starts; the later `graphical-session.target` on
  GNOME/KDE finds it already active → no double start, no restart.
- **`DISPLAY`/`XAUTHORITY` arrive via `pam_systemd`** at login on GDM/lightdm/SDDM alike
  (verified on this MX/lightdm box with `systemctl --user show-environment`), so the daemon
  connects at `default.target` without hardcoding a display. Re-verify on KDE/SDDM and
  GNOME/GDM before assuming the same.
- **`PartOf=graphical-session.target`** gives a clean stop at GNOME/KDE logout; on xfce it
  never fires (target never runs) — the daemon ends with the user manager either way.
- **Verify when KDE/GNOME testing starts:**
  1. exactly one start at login (no double instance — second would lose the `Super+T` grab);
  2. `Super+T` conflicts: KDE `KGlobalAccel` and GNOME keyboard shortcuts may own the key —
     the grab then fails with `GrabKey` Access warnings (degrade-by-design, corner still
     works). M5/S06 config should make the shortcut rebindable;
  3. GNOME Wayland: XWayland `DISPLAY` is set, but §17.3 stacking degradation applies
     (overlay never above native Wayland fullscreen surfaces — GNOME Wayland is
     unreachable for the core requirement per §17.3).

## Script hardening — collision review (follow-up to this fix)

Reviewed all five scripts for cross-script collisions; fixed four issues:

1. **upgrade.sh had no dev guard** (install.sh had one): running it over a dev session
   would `pkill` the dev instance and install a fresh binary over the stashed one,
   dangling the swap state. upgrade.sh now refuses identically to install.sh (state
   file present or a dev `hover-clock` process whose exe differs from the install
   target).
2. **swap-to-dev.sh run twice** would overwrite the `state` file and orphan the stashed
   production binary (swap-to-prod could no longer restore it). It now refuses when the
   state file already exists.
3. **No concurrency protection**: all five scripts now take a non-blocking flock on
   `~/.local/state/hover-clock/lock` and refuse with a clear message if another
   hover-clock script holds it. swap-to-dev releases the lock before `exec cargo run`,
   so uninstall can still stop a dev session (documented behavior).
4. **install.sh guard compared raw paths** — canonicalized via `readlink -f` both sides
   so a symlinked `BIN_DIR` cannot false-positive as a dev instance.

Verified: `bash -n` on all scripts; lock-contention refusal; state-file refusal across
install/upgrade/swap-to-dev; full dev round-trip (swap-to-dev → stash + state,
swap-to-prod → restore + re-enable + daemon active).

## Observations (not fixed)

1. **`Super + T` grab conflict at one login** (2026-08-16 05:30:10): the daemon logged
   `GrabKey` Access errors ×4 (all lock-state combos) then kept running — corner activation
   unaffected. Non-fatal by design (facade degrades to warnings). Subsequent starts grabbed
   cleanly; conflict was transient and not reproduced. If it recurs, check whether another
   client (xfce keyboard shortcuts, a stale dev instance) holds `Super + T` at login.
2. **4 px hot corner** (M2 decision, recorded in `03-m2-activation.md`): on this machine the
   Synaptics touchpad generates continuous real motion, which drifts the pointer out of the
   4×4 px corner before the 200 ms dwell completes — corner dwell is practically unusable
   with a live touchpad. Revisit corner size with S04's placement/config work (§15 open
   questions list corner geometry/timing as open).
3. **`Restart=on-failure` + default start-limit** (5 starts / 10 s): fine for DM logins (X is
   up before the user manager). A console-login + manual `startx` user could hit the limit
   while X is not yet up; consider `StartLimitIntervalSec=0` if that setup needs support.
4. Exit-code-1 entries at previous boot ends coincide with **shutdown/session teardown**
   (X connection drops first, GTK exits 1) — benign, not a crash loop.

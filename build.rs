//! Dev-workflow guard (AGENTS.md — Dev/prod swap discipline).
//!
//! Aborts a *debug* build from the repo unless a dev session is active
//! (`swap-to-dev.sh` wrote its state file). A bare `cargo run` started
//! outside the swap scripts leaves a daemon running with no swap state,
//! so `swap-to-prod.sh` cannot restore production — the guard makes that
//! state impossible to create by accident.
//!
//! The legitimate non-dev build paths bypass the guard:
//!   - release builds (`install.sh` / `upgrade.sh` build `--release`)
//!   - CI (GitHub Actions sets `CI`; a runner has no swap state)
//!   - `HOVERCLOCK_BYPASS_DEV_GUARD=1` (scripted builds, e.g. `deploy.sh`
//!     syncing Cargo.lock)
//!
//! The guard is only armed on machines that have the install/swap
//! machinery at all (the state directory exists): a fresh clone with no
//! production install has nothing to protect, so it builds freely.

use std::env;
use std::path::PathBuf;
use std::process;

fn main() {
    if env::var("PROFILE").as_deref() == Ok("release") {
        return;
    }
    if env::var_os("CI").is_some() || env::var_os("HOVERCLOCK_BYPASS_DEV_GUARD").is_some() {
        return;
    }
    let Some(state_dir) = state_dir() else {
        return; // cannot locate the swap state — do not block unknown environments
    };
    if !state_dir.is_dir() {
        return; // no install/swap machinery on this machine — nothing to protect
    }
    if state_dir.join("state").is_file() {
        return; // dev session active (swap-to-dev ran)
    }
    eprintln!();
    eprintln!("error: dev build blocked — swap-to-dev was not run before this build.");
    eprintln!();
    eprintln!("Building/running from source is only allowed inside a dev session");
    eprintln!(
        "(swap state: {}). Start one with:",
        state_dir.join("state").display()
    );
    eprintln!();
    eprintln!("    ./scripts/swap-to-dev.sh      # stashes the installed binary, runs from source");
    eprintln!();
    eprintln!("and return to production with:");
    eprintln!();
    eprintln!("    ./scripts/swap-to-prod.sh");
    eprintln!();
    eprintln!("Release builds (install.sh/upgrade.sh), CI, and");
    eprintln!("HOVERCLOCK_BYPASS_DEV_GUARD=1 bypass this guard.");
    process::exit(1);
}

/// `~/.local/state/hover-clock` (or `$XDG_STATE_HOME/hover-clock`).
fn state_dir() -> Option<PathBuf> {
    if let Some(xdg) = env::var_os("XDG_STATE_HOME") {
        return Some(PathBuf::from(xdg).join("hover-clock"));
    }
    let home = env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".local/state/hover-clock"))
}

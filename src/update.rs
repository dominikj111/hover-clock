//! Self-update (roadmap S09): download the release tarball for the
//! current architecture, verify its SHA-256, extract it, atomically
//! replace the running binary, and restart the daemon.
//!
//! Runs on a worker thread (blocking HTTP + file I/O); the caller
//! reports progress on the main loop. Any failure before the binary is
//! replaced leaves the running install untouched — the update is
//! all-or-nothing at the replace step (staged copy + atomic rename).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use crate::version::Release;

/// Download budget — the tarball is a few hundred KB; generous read
/// timeout for slower connections.
const DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Perform the update for a release. On success the daemon is restarted
/// (systemd) or a detached re-exec helper is spawned and this process
/// exits — callers should not expect to keep running.
pub fn run(release: &Release) -> Result<String, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(5))
        .timeout_read(DOWNLOAD_TIMEOUT)
        .build();

    // 1. Download the tarball + checksum into a private temp dir.
    let work_dir = std::env::temp_dir().join(format!("hover-clock-update-{}", std::process::id()));
    std::fs::create_dir_all(&work_dir)
        .map_err(|err| format!("cannot create update work dir: {err}"))?;
    let tarball = work_dir.join("release.tar.gz");
    let checksum = work_dir.join("release.sha256");
    download(&agent, &release.tarball_url, &tarball)?;
    download(&agent, &release.sha256_url, &checksum)?;

    // 2. Verify the checksum — a mismatch aborts before anything is
    //    replaced.
    let expected = parse_checksum(
        &std::fs::read_to_string(&checksum)
            .map_err(|err| format!("cannot read downloaded checksum: {err}"))?,
    )?;
    let actual = sha256_file(&tarball)?;
    if !actual.eq_ignore_ascii_case(&expected) {
        let _ = std::fs::remove_dir_all(&work_dir);
        return Err(format!(
            "checksum mismatch ({actual} ≠ {expected}) — update aborted"
        ));
    }

    // 3. Extract (system tar — guaranteed on Linux, no extra crate deps).
    let extract_dir = work_dir.join("extracted");
    std::fs::create_dir_all(&extract_dir)
        .map_err(|err| format!("cannot create extract dir: {err}"))?;
    let status = Command::new("tar")
        .arg("xzf")
        .arg(&tarball)
        .arg("-C")
        .arg(&extract_dir)
        .status()
        .map_err(|err| format!("cannot run tar: {err}"))?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(&work_dir);
        return Err("tarball extraction failed".into());
    }
    let new_binary = extract_dir.join("hover-clock");
    if !new_binary.is_file() {
        let _ = std::fs::remove_dir_all(&work_dir);
        return Err("release tarball has no hover-clock binary".into());
    }

    // 4. Replace the running binary atomically: stage next to it, then
    //    rename over it. The running process keeps its old inode until
    //    the restart.
    let current =
        std::env::current_exe().map_err(|err| format!("cannot find own binary: {err}"))?;
    let staged = staged_path(&current);
    std::fs::copy(&new_binary, &staged).map_err(|err| format!("cannot stage new binary: {err}"))?;
    let mut perms = std::fs::metadata(&staged)
        .map_err(|err| format!("cannot stat staged binary: {err}"))?
        .permissions();
    use std::os::unix::fs::PermissionsExt;
    perms.set_mode(0o755);
    std::fs::set_permissions(&staged, perms)
        .map_err(|err| format!("cannot chmod staged binary: {err}"))?;
    std::fs::rename(&staged, &current).map_err(|err| format!("cannot replace binary: {err}"))?;

    // 5. Restart: systemd restarts the unit (stop then start — the
    //    control socket is released before the new process binds); a
    //    non-systemd install spawns a detached helper that re-execs this
    //    binary after this process exits, then we exit.
    let _ = std::fs::remove_dir_all(&work_dir);
    restart(&current)?;
    Ok("updated — restarting".into())
}

/// A sibling name for the staged binary (`hover-clock.new` next to
/// `hover-clock`), so the atomic rename stays on the same filesystem.
fn staged_path(current: &Path) -> PathBuf {
    let mut staged = current.as_os_str().to_owned();
    staged.push(".new");
    PathBuf::from(staged)
}

fn download(agent: &ureq::Agent, url: &str, path: &Path) -> Result<(), String> {
    let response = agent
        .get(url)
        .set(
            "User-Agent",
            concat!("hover-clock/", env!("CARGO_PKG_VERSION")),
        )
        .call()
        .map_err(|err| format!("cannot download {url}: {err}"))?;
    let mut file = std::fs::File::create(path)
        .map_err(|err| format!("cannot write download to {path:?}: {err}"))?;
    std::io::copy(&mut response.into_reader(), &mut file)
        .map_err(|err| format!("cannot save download to {path:?}: {err}"))?;
    file.flush()
        .map_err(|err| format!("cannot flush download: {err}"))?;
    Ok(())
}

/// First whitespace-separated token of a `sha256sum`-format file
/// (`<hex>  <filename>`).
fn parse_checksum(text: &str) -> Result<String, String> {
    text.split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or_else(|| "checksum file is empty".into())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file =
        std::fs::File::open(path).map_err(|err| format!("cannot open {path:?}: {err}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|err| format!("cannot read {path:?}: {err}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Restart the daemon. systemd path: `systemctl --user restart` stops the
/// unit (SIGTERM — the old process releases the control socket and GTK's
/// shutdown hook unlinks it) then starts the new binary — used only when
/// the unit actually exists *and* is enabled (dev mode disables it).
/// Otherwise a detached `sh -c "sleep 1; exec … --start"` waits for this
/// process to exit before the new one binds the socket, avoiding the
/// single-instance guard; the new process reclaims the stale socket file.
fn restart(binary: &Path) -> Result<(), String> {
    let unit = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(".config/systemd/user/hover-clock.service");
    if unit.is_file() && unit_enabled() {
        let status = Command::new("systemctl")
            .args(["--user", "restart", "hover-clock"])
            .status()
            .map_err(|err| format!("cannot run systemctl: {err}"))?;
        if status.success() {
            return Ok(());
        }
    }
    let script = format!("sleep 1; exec {} --start", binary.display());
    Command::new("sh")
        .arg("-c")
        .arg(&script)
        .spawn()
        .map_err(|err| format!("cannot spawn re-exec helper: {err}"))?;
    std::process::exit(0);
}

/// Is the systemd user unit currently enabled? (Enabled = the production
/// daemon the unit starts; disabled in dev mode — the stashed binary the
/// unit points at is missing, so `systemctl restart` would fail.)
fn unit_enabled() -> bool {
    Command::new("systemctl")
        .args(["--user", "is-enabled", "hover-clock"])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sha256sum_format() {
        assert_eq!(
            parse_checksum("abc123  hover-clock-v1.0.0-x86_64.tar.gz\n"),
            Ok("abc123".into())
        );
        assert_eq!(parse_checksum(""), Err("checksum file is empty".into()));
    }

    #[test]
    fn stages_binary_next_to_current() {
        assert_eq!(
            staged_path(Path::new("/home/u/.local/bin/hover-clock")),
            PathBuf::from("/home/u/.local/bin/hover-clock.new")
        );
    }
}

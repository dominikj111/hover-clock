//! Version label source: the running binary's version vs the latest
//! published release on GitHub (proposal §11.2; roadmap S09).
//!
//! The check runs on a worker thread (blocking HTTP with timeouts) and
//! the result crosses back to the GTK main loop via `MainContext::invoke`
//! — the UI thread never blocks on network I/O. Failed checks (offline,
//! rate-limited, malformed response) degrade to "no newer version known":
//! the label keeps the running version in its current colour, never an
//! error.

use std::fmt;
use std::time::Duration;

/// Connect/read budget for the release check — bounded so the worker
/// thread always finishes and the daemon never accumulates stuck checks.
const HTTP_TIMEOUT: Duration = Duration::from_secs(5);

/// A numeric `x.y.z` version. Numeric-only by design; pre-release/build
/// metadata is not part of the comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    major: u32,
    minor: u32,
    patch: u32,
}

impl Version {
    fn parse(text: &str) -> Option<Self> {
        let mut parts = text.trim().split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        Some(Self {
            major,
            minor,
            patch,
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The version of this running binary.
pub fn running_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("CARGO_PKG_VERSION is always x.y.z")
}

/// `v<current>` label shown before the network check has an answer.
pub fn current_label() -> String {
    format!("v{}", running_version())
}

/// A published release, with the assets the self-update needs (roadmap
/// S09): the tarball for the current architecture and its checksum file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub tarball_url: String,
    pub sha256_url: String,
}

/// The latest published release of this repository, with the current
/// architecture's assets, if it can be determined. `None` on any failure
/// — offline, rate-limited, timeouts, malformed response — and callers
/// degrade gracefully.
pub fn latest_release() -> Option<Release> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(HTTP_TIMEOUT)
        .timeout_read(HTTP_TIMEOUT)
        .build();
    let body = agent
        .get(&release_api_url()?)
        .set(
            "User-Agent",
            concat!("hover-clock/", env!("CARGO_PKG_VERSION")),
        )
        .set("Accept", "application/vnd.github+json")
        .call()
        .ok()?
        .into_string()
        .ok()?;
    release_from_json(&body)
}

/// Parse a release + asset URLs from the `releases/latest` JSON.
fn release_from_json(body: &str) -> Option<Release> {
    let version = tag_version(body)?;
    let arch = std::env::consts::ARCH; // matches the release asset names
    let tarball = format!("hover-clock-v{version}-{arch}.tar.gz");
    Some(Release {
        version,
        tarball_url: asset_url(body, &tarball)?,
        sha256_url: asset_url(body, &format!("{tarball}.sha256"))?,
    })
}

/// Find the `browser_download_url` of the asset with the given `name` in
/// the releases JSON (lightweight — no JSON parser needed for one field
/// pairing; the API shape is stable).
fn asset_url(body: &str, name: &str) -> Option<String> {
    let key = format!("\"name\":\"{name}\"");
    let start = body.find(&key)?;
    let url_key = "\"browser_download_url\":\"";
    let url_start = body[start..].find(url_key)? + start + url_key.len();
    let url_end = body[url_start..].find('"')? + url_start;
    Some(body[url_start..url_end].to_string())
}

/// GitHub Releases API URL derived from the repository URL in Cargo.toml
/// (`https://github.com/owner/repo` → `https://api.github.com/repos/owner/repo/...`).
fn release_api_url() -> Option<String> {
    let path = env!("CARGO_PKG_REPOSITORY").strip_prefix("https://github.com/")?;
    let (owner, name) = path.split_once('/')?;
    Some(format!(
        "https://api.github.com/repos/{owner}/{name}/releases/latest"
    ))
}

/// Extract the `tag_name` ("v1.2.3") from the `releases/latest` JSON.
fn tag_version(body: &str) -> Option<Version> {
    let key = "\"tag_name\":\"";
    let start = body.find(key)? + key.len();
    let end = body[start..].find('"')? + start;
    let tag = body[start..end]
        .strip_prefix('v')
        .unwrap_or(&body[start..end]);
    Version::parse(tag)
}

/// Pure label logic, testable without the network: `v<current>` by
/// default, `v<current> → v<latest>` when a newer release exists. The
/// boolean is true when the label should warn (orange).
pub fn label_text(current: Version, latest: Option<Version>) -> (String, bool) {
    match latest {
        Some(latest) if latest > current => (format!("v{current} → v{latest}"), true),
        _ => (format!("v{current}"), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_numeric_versions() {
        assert_eq!(
            Version::parse("0.1.0"),
            Some(Version {
                major: 0,
                minor: 1,
                patch: 0
            })
        );
        assert_eq!(
            Version::parse("1.20.3"),
            Some(Version {
                major: 1,
                minor: 20,
                patch: 3
            })
        );
        assert_eq!(Version::parse("0.1"), None);
        assert_eq!(Version::parse("nope"), None);
        assert_eq!(Version::parse(""), None);
    }

    #[test]
    fn orders_versions() {
        assert!(Version::parse("0.2.0").unwrap() > Version::parse("0.1.9").unwrap());
        assert!(Version::parse("1.0.0").unwrap() > Version::parse("0.9.9").unwrap());
        assert_eq!(
            Version::parse("0.1.0").unwrap(),
            Version::parse("0.1.0").unwrap()
        );
    }

    #[test]
    fn parses_tag_name_from_release_json() {
        let body = r#"{"url":"...","tag_name":"v1.0.0","name":"1.0.0","draft":false}"#;
        assert_eq!(
            tag_version(body),
            Some(Version {
                major: 1,
                minor: 0,
                patch: 0
            })
        );
        assert_eq!(tag_version("{}"), None);
        assert_eq!(tag_version(""), None);
    }

    #[test]
    fn parses_release_with_arch_assets() {
        let arch = std::env::consts::ARCH;
        let body = format!(
            r#"{{"tag_name":"v1.1.0","assets":[
                {{"name":"hover-clock-v1.1.0-{arch}.tar.gz","browser_download_url":"https://github.com/dominikj111/hover-clock/releases/download/v1.1.0/hover-clock-v1.1.0-{arch}.tar.gz"}},
                {{"name":"hover-clock-v1.1.0-{arch}.tar.gz.sha256","browser_download_url":"https://github.com/dominikj111/hover-clock/releases/download/v1.1.0/hover-clock-v1.1.0-{arch}.tar.gz.sha256"}}
            ]}}"#
        );
        let release = release_from_json(&body).expect("parses");
        assert_eq!(release.version, Version::parse("1.1.0").unwrap());
        assert!(
            release
                .tarball_url
                .ends_with(&format!("hover-clock-v1.1.0-{arch}.tar.gz")),
            "got {}",
            release.tarball_url
        );
        assert!(release.sha256_url.ends_with(".sha256"));
        // A release missing the current arch's assets is not updatable.
        let other_arch = if arch == "x86_64" {
            "aarch64"
        } else {
            "x86_64"
        };
        let other = format!(
            r#"{{"tag_name":"v1.1.0","assets":[{{"name":"hover-clock-v1.1.0-{other_arch}.tar.gz","browser_download_url":"u"}}]}}"#
        );
        assert_eq!(release_from_json(&other), None);
    }

    #[test]
    fn derives_release_api_url_from_repository() {
        assert_eq!(
            release_api_url(),
            Some("https://api.github.com/repos/dominikj111/hover-clock/releases/latest".into())
        );
    }

    #[test]
    fn label_shows_upgrade_only_when_latest_is_newer() {
        let current = Version::parse("1.0.0").unwrap();
        assert_eq!(label_text(current, None), ("v1.0.0".into(), false));
        assert_eq!(
            label_text(current, Some(Version::parse("1.0.0").unwrap())),
            ("v1.0.0".into(), false)
        );
        let newer = label_text(current, Some(Version::parse("1.1.0").unwrap()));
        assert_eq!(newer, ("v1.0.0 → v1.1.0".into(), true));
        // A release behind the running binary is not an upgrade.
        assert_eq!(
            label_text(Version::parse("1.1.0").unwrap(), Some(current)),
            ("v1.1.0".into(), false)
        );
    }
}

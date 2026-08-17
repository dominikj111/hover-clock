//! Version label source (interim): the running binary's version vs the
//! version in a local `Cargo.toml`.
//!
//! The real update check (against GitHub releases) is future work; until
//! then the overlay shows whether the repository has moved on from the
//! binary currently running on the system. The repository is located by
//! `$HOVERCLOCK_SOURCE_DIR` first, then the working directory and its
//! ancestors — so dev runs from the repo root work without setup, and an
//! installed daemon can be pointed at the source explicitly.

use std::path::PathBuf;

/// A numeric `x.y.z` version. Numeric-only by design for the interim
/// check; full semver (pre-release/build metadata) lands with the real
/// update check.
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

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Parse the `version` of the `[package]` section from `Cargo.toml`
/// text (dependencies also carry `version =` lines — anchor on the
/// section, not the first match).
fn parse_cargo_version(text: &str) -> Option<Version> {
    let lines: Vec<&str> = text.lines().collect();
    let package = lines.iter().position(|line| line.trim() == "[package]")?;
    let line = lines[package..]
        .iter()
        .find(|line| line.trim_start().starts_with("version ="))?;
    let value = line.split('=').nth(1)?.trim().trim_matches('"');
    Version::parse(value)
}

/// Locate a local `Cargo.toml`: `$HOVERCLOCK_SOURCE_DIR` override, then
/// the working directory and up to four ancestors (dev runs start from
/// the repository root).
fn find_cargo_toml() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("HOVERCLOCK_SOURCE_DIR") {
        let candidate = PathBuf::from(dir).join("Cargo.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let cwd = std::env::current_dir().ok()?;
    let mut dir = Some(cwd.as_path());
    for _ in 0..5 {
        let candidate = dir?.join("Cargo.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = dir?.parent();
    }
    None
}

/// The version line for the overlay: `v<current>` by default, `v<current>
/// → v<repo>` when a newer version exists in the local repository. The
/// boolean is true when the label should warn (orange).
pub fn version_label() -> (String, bool) {
    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .expect("CARGO_PKG_VERSION is always a valid x.y.z version");
    let repo = find_cargo_toml()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| parse_cargo_version(&text));
    status(current, repo)
}

/// Pure label logic, testable without the filesystem.
fn status(current: Version, repo: Option<Version>) -> (String, bool) {
    match repo {
        Some(repo) if repo > current => (format!("v{current} → v{repo}"), true),
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
    fn parses_package_version_from_cargo_toml() {
        let toml = "[package]\nname = \"hover-clock\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nchrono = \"0.4\"\ngtk = { version = \"0.11.4\", package = \"gtk4\" }\n";
        assert_eq!(
            parse_cargo_version(toml),
            Some(Version {
                major: 0,
                minor: 1,
                patch: 0
            })
        );
    }

    #[test]
    fn ignores_dependency_versions_without_package_section() {
        let toml = "[dependencies]\nchrono = { version = \"0.4.45\" }\n";
        assert_eq!(parse_cargo_version(toml), None);
    }

    #[test]
    fn label_shows_upgrade_only_when_repo_is_newer() {
        let current = Version::parse("0.1.0").unwrap();
        assert_eq!(status(current, None), ("v0.1.0".into(), false));
        assert_eq!(
            status(current, Some(Version::parse("0.1.0").unwrap())),
            ("v0.1.0".into(), false)
        );
        let newer = status(current, Some(Version::parse("0.2.0").unwrap()));
        assert_eq!(newer, ("v0.1.0 → v0.2.0".into(), true));
        // A repo behind the running binary is not an upgrade.
        assert_eq!(
            status(Version::parse("0.2.0").unwrap(), Some(current)),
            ("v0.2.0".into(), false)
        );
    }
}

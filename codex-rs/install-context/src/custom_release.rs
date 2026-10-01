//! GitHub-only release identity for the custom distribution.

pub const CUSTOM_RELEASE_URL: &str = "https://github.com/SeventyfourSam/codex/releases/latest";
pub const CUSTOM_RELEASE_API: &str =
    "https://api.github.com/repos/SeventyfourSam/codex/releases/latest";
pub const CUSTOM_VERSION_CACHE: &str = "custom-version.json";
pub const CUSTOM_INSTALL_UNIX: &str =
    "https://github.com/SeventyfourSam/codex/releases/latest/download/install.sh";
pub const CUSTOM_INSTALL_WINDOWS: &str =
    "https://github.com/SeventyfourSam/codex/releases/latest/download/install.ps1";

/// A publish is usable only after both platform packages and checksums are present.
#[derive(serde::Deserialize)]
pub struct CustomRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<ReleaseAsset>,
}

#[derive(serde::Deserialize)]
struct ReleaseAsset {
    name: String,
}

impl CustomRelease {
    pub fn version(&self) -> Option<&str> {
        let version = custom_version_from_tag(&self.tag_name)?;
        let required = [
            format!("codex-{version}-aarch64-apple-darwin.tar.gz"),
            format!("codex-{version}-x86_64-pc-windows-msvc.zip"),
            "SHA256SUMS".to_string(),
        ];
        (!self.draft
            && !self.prerelease
            && required
                .iter()
                .all(|name| self.assets.iter().any(|asset| &asset.name == name)))
        .then_some(version)
    }
}

/// Parse the upstream release and numeric fork revision without changing protocol versions.
pub fn parse_custom_version(version: &str) -> Option<[u64; 4]> {
    let (base, revision) = match version.strip_suffix("-custom") {
        Some(base) => (base, "0"),
        None => version.split_once("-custom.")?,
    };
    let mut components = base.split('.').chain(std::iter::once(revision));
    let mut parsed = [0; 4];
    for value in &mut parsed {
        let component = components.next()?;
        if component.is_empty()
            || !component.bytes().all(|byte| byte.is_ascii_digit())
            || component.len() > 1 && component.starts_with('0')
        {
            return None;
        }
        *value = component.parse().ok()?;
    }
    components.next().is_none().then_some(parsed)
}

/// Return a fork version only for a valid custom release tag.
pub fn custom_version_from_tag(tag: &str) -> Option<&str> {
    let version = tag.strip_prefix('v')?;
    parse_custom_version(version)?;
    Some(version)
}

#[cfg(test)]
#[path = "custom_release_tests.rs"]
mod tests;

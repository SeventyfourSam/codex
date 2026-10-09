//! Custom release selection without removing upstream package-manager update actions.
#![cfg_attr(debug_assertions, allow(dead_code, unused_imports))]

use crate::update_action::UpdateAction;

#[cfg(any(not(debug_assertions), test))]
mod cache;
#[cfg(any(not(debug_assertions), test))]
mod check;
#[cfg(any(not(debug_assertions), test))]
pub(crate) use check::get_upgrade_version;
#[cfg(any(not(debug_assertions), test))]
pub(crate) use check::get_upgrade_version_for_popup;

pub(crate) fn enabled() -> bool {
    // Upstream's interactive updater is also disabled in development builds.
    !cfg!(debug_assertions)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CustomUpdate {
    Unix,
    Windows,
}

impl CustomUpdate {
    pub fn command_args(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Self::Unix => (
                "sh",
                &[
                    "-c",
                    "set -e; script=$(mktemp); trap 'rm -f \"$script\"' EXIT; curl -fsSL --connect-timeout 10 --max-time 30 https://github.com/SeventyfourSam/codex/releases/latest/download/install.sh -o \"$script\"; sh \"$script\"",
                ],
            ),
            Self::Windows => (
                "powershell",
                &[
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-c",
                    "$ErrorActionPreference = 'Stop'; irm -TimeoutSec 30 https://github.com/SeventyfourSam/codex/releases/latest/download/install.ps1 | iex",
                ],
            ),
        }
    }
}

pub(crate) fn get_update_action() -> Option<UpdateAction> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some(UpdateAction::StandaloneUnix)
    } else if cfg!(all(windows, target_arch = "x86_64")) {
        Some(UpdateAction::StandaloneWindows)
    } else {
        None
    }
}

pub(crate) fn command_args(
    action: UpdateAction,
) -> Option<(&'static str, &'static [&'static str])> {
    if !enabled() {
        return None;
    }
    match action {
        UpdateAction::StandaloneUnix => Some(CustomUpdate::Unix.command_args()),
        UpdateAction::StandaloneWindows => Some(CustomUpdate::Windows.command_args()),
        _ => None,
    }
}

pub(crate) fn current_version(action: UpdateAction) -> &'static str {
    if command_args(action).is_some() {
        codex_utils_cli::CUSTOM_VERSION
    } else {
        crate::version::CODEX_CLI_VERSION
    }
}

pub(crate) fn release_notes_url(action: UpdateAction, upstream: &'static str) -> &'static str {
    if command_args(action).is_some() {
        codex_install_context::CUSTOM_RELEASE_URL
    } else {
        upstream
    }
}

pub(crate) fn is_newer(latest: &str, current: &str) -> Option<bool> {
    match (
        codex_install_context::parse_custom_version(latest),
        codex_install_context::parse_custom_version(current),
    ) {
        (Some(latest), Some(current)) => Some(latest > current),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_install_commands_use_only_the_fork_release_with_timeouts() {
        for action in [CustomUpdate::Unix, CustomUpdate::Windows] {
            let (_, args) = action.command_args();
            let command = args.join(" ");
            assert!(command.contains(
                "https://github.com/SeventyfourSam/codex/releases/latest/download/install."
            ));
            assert!(!command.contains("chatgpt.com/codex"));
            assert!(command.contains("30"));
        }
    }
}

/// Update the custom GitHub distribution after restoring the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateAction {
    Daemon(DaemonUpdateSource),
    StandaloneUnix,
    StandaloneWindows,
}

impl UpdateAction {
    pub fn command_args(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Self::Daemon(source) => ("codex", source.command_args()),
            Self::StandaloneUnix => (
                "sh",
                &[
                    "-c",
                    "set -e; script=$(mktemp); trap 'rm -f \"$script\"' EXIT; curl -fsSL --connect-timeout 10 --max-time 30 https://github.com/SeventyfourSam/codex/releases/latest/download/install.sh -o \"$script\"; sh \"$script\"",
                ],
            ),
            Self::StandaloneWindows => (
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

    pub fn command_str(self) -> String {
        if !matches!(self, Self::Daemon(_)) {
            return "codex update".to_string();
        }
        let (command, args) = self.command_args();
        shlex::try_join(std::iter::once(command).chain(args.iter().copied()))
            .unwrap_or_else(|_| format!("{command} {}", args.join(" ")))
    }
}

#[cfg(any(not(debug_assertions), test))]
pub fn get_update_action() -> Option<UpdateAction> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some(UpdateAction::StandaloneUnix)
    } else if cfg!(all(windows, target_arch = "x86_64")) {
        Some(UpdateAction::StandaloneWindows)
    } else {
        None
    }
}

/// Package source explicitly selected by the user in the daemon menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonUpdateSource {
    PublicStable,
    ThisCli,
}

impl DaemonUpdateSource {
    pub fn command_args(self) -> &'static [&'static str] {
        match self {
            Self::PublicStable => &["app-server", "daemon", "update"],
            Self::ThisCli => &["app-server", "daemon", "update", "--from-cli", "--yes"],
        }
    }
}

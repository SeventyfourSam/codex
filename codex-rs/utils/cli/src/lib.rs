mod approval_mode_cli_arg;
mod config_override;
pub(crate) mod format_env_display;
mod resume_command;
mod sandbox_mode_cli_arg;
mod shared_options;

pub use approval_mode_cli_arg::ApprovalModeCliArg;
pub use codex_protocol::config_types::ProfileV2Name;
pub use config_override::CliConfigOverrides;
pub use format_env_display::format_env_display;
pub use resume_command::resume_command;
pub use resume_command::resume_hint;
pub use sandbox_mode_cli_arg::SandboxModeCliArg;
pub use shared_options::SharedCliOptions;

/// Fork version for explicit custom-build queries, independent of upstream version logic.
/// Unset or empty `CODEX_CUSTOM_REVISION` produces `-custom` (revision zero).
/// An explicit revision preserves the numbered `-custom.N` spelling.
pub const CUSTOM_VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), env!("CODEX_CUSTOM_SUFFIX"));

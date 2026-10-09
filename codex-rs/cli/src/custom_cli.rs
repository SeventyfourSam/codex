//! Private installer switches without additions to the upstream clap argument struct.

pub(crate) fn enabled() -> bool {
    true
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

pub(super) fn handle_private_switches() -> anyhow::Result<bool> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let custom = args.first().is_some_and(|arg| arg == "--custom");
    let initialize = args
        .first()
        .is_some_and(|arg| arg == "--initialize-custom-config");
    if !custom && !initialize {
        return Ok(false);
    }
    anyhow::ensure!(
        args.len() == 1,
        "custom installer switches must be used alone"
    );
    if custom {
        println!("codex-cli {}", codex_utils_cli::CUSTOM_VERSION);
    } else {
        codex_config::initialize_custom_config(&codex_core::config::find_codex_home()?)?;
    }
    Ok(true)
}

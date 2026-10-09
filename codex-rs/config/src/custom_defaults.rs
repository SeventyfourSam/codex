//! Install the custom distribution's defaults using ordinary user configuration.

use std::io;
use std::path::Path;

use codex_utils_path::resolve_symlink_write_paths;
use codex_utils_path::write_atomically;
use toml_edit::DocumentMut;
use toml_edit::Item;
use toml_edit::Table;
use toml_edit::value;

/// Fill missing custom settings without replacing explicit values or other configuration.
///
/// Installers call this on the validated new binary before changing the active package.
/// Malformed TOML or incompatible table shapes fail without writing the configuration.
pub fn initialize_custom_config(codex_home: &Path) -> io::Result<()> {
    let paths = resolve_symlink_write_paths(&codex_home.join(crate::CONFIG_TOML_FILE))?;
    let original = match paths.read_path.as_deref().map(std::fs::read_to_string) {
        Some(Ok(text)) => text,
        None => String::new(),
        Some(Err(error)) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Some(Err(error)) => return Err(error),
    };
    let mut document = original
        .parse::<DocumentMut>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "config.toml is not valid TOML"))?;
    document
        .as_table_mut()
        .entry("check_for_update_on_startup")
        .or_insert(value(/*v*/ false));
    for (section, defaults) in [
        ("analytics", vec![("enabled", value(/*v*/ false))]),
        ("feedback", vec![("enabled", value(/*v*/ false))]),
        (
            "otel",
            vec![
                ("exporter", value("none")),
                ("trace_exporter", value("none")),
                ("metrics_exporter", value("none")),
            ],
        ),
    ] {
        let item = document
            .as_table_mut()
            .entry(section)
            .or_insert(Item::Table(Table::new()));
        let table = item.as_table_like_mut().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("config.toml: {section} must be a table"),
            )
        })?;
        for (key, default) in defaults {
            if !table.contains_key(key) {
                table.insert(key, default);
            }
        }
    }
    let updated = document.to_string();
    let daemon_defaults = prepare_daemon_defaults(codex_home)?;
    if updated != original {
        write_atomically(&paths.write_path, &updated)?;
    }
    if let Some((path, settings)) = daemon_defaults {
        write_atomically(&path, &settings)?;
    }
    Ok(())
}

// Keep daemon defaults in installed configuration instead of changing upstream
// DaemonSettings, its deserializer, and all tests that depend on their defaults.
fn prepare_daemon_defaults(codex_home: &Path) -> io::Result<Option<(std::path::PathBuf, String)>> {
    let paths = resolve_symlink_write_paths(&codex_home.join("app-server-daemon/settings.json"))?;
    let original = match paths.read_path.as_deref().map(std::fs::read_to_string) {
        Some(Ok(text)) => text,
        None => String::new(),
        Some(Err(error)) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Some(Err(error)) => return Err(error),
    };
    let mut settings: serde_json::Value = if original.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(&original).map_err(io::Error::other)?
    };
    let root = settings.as_object_mut().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "daemon settings must be an object",
        )
    })?;
    let updater = root
        .entry("updater")
        .or_insert_with(|| serde_json::json!({}));
    let updater = updater.as_object_mut().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "daemon updater must be an object",
        )
    })?;
    if updater.contains_key("autoUpdateEnabled") {
        return Ok(None);
    }
    updater.insert("autoUpdateEnabled".to_string(), false.into());
    let updated = serde_json::to_string_pretty(&settings).map_err(io::Error::other)? + "\n";
    Ok(Some((paths.write_path, updated)))
}

#[cfg(test)]
#[path = "custom_defaults_tests.rs"]
mod tests;

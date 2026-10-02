use anyhow::Result;
use pretty_assertions::assert_eq;
use std::process::Command;

#[test]
fn installer_initializes_config_without_starting_a_session() -> Result<()> {
    let home = tempfile::tempdir()?;
    let path = home.path().join("config.toml");
    // An unknown provider is harmless here: initialization must not load a session.
    std::fs::write(&path, "model_provider = 'not-installed'\n")?;
    let binary = codex_utils_cargo_bin::cargo_bin("codex")?;
    for _ in 0..2 {
        let output = Command::new(&binary)
            .env("CODEX_HOME", home.path())
            .arg("--initialize-custom-config")
            .output()?;
        assert!(output.status.success(), "{output:?}");
        assert!(output.stdout.is_empty());
    }
    let contents = std::fs::read_to_string(&path)?;
    let config: toml::Value = toml::from_str(&contents)?;
    assert_eq!(config["model_provider"].as_str(), Some("not-installed"));
    assert_eq!(config["feedback"]["enabled"].as_bool(), Some(false));
    assert!(!home.path().join("sessions").exists());

    std::fs::write(&path, "invalid = [")?;
    let output = Command::new(binary)
        .env("CODEX_HOME", home.path())
        .arg("--initialize-custom-config")
        .output()?;
    assert!(!output.status.success());
    assert_eq!(std::fs::read_to_string(path)?, "invalid = [");
    Ok(())
}

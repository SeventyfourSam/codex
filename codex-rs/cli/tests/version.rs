use anyhow::Result;
use pretty_assertions::assert_eq;
use std::process::Command;

#[test]
fn custom_version_is_separate_from_the_upstream_version() -> Result<()> {
    let home = tempfile::tempdir()?;
    // Version queries must exit before loading configuration or starting a session.
    std::fs::write(home.path().join("config.toml"), "invalid = [")?;
    let mut rendered = String::new();
    for (flag, version) in [
        ("--version", env!("CARGO_PKG_VERSION")),
        ("--custom", codex_utils_cli::CUSTOM_VERSION),
    ] {
        let output = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?)
            .env("CODEX_HOME", home.path())
            .arg(flag)
            .output()?;
        assert!(output.status.success(), "{output:?}");
        let stdout = String::from_utf8(output.stdout)?;
        assert_eq!(stdout, format!("codex-cli {version}\n"));
        rendered.push_str(&format!("$ codex {flag}\n{stdout}"));
    }
    let rendered = rendered
        .replace(
            codex_utils_cli::CUSTOM_VERSION,
            "<UPSTREAM>-custom.<REVISION>",
        )
        .replace(env!("CARGO_PKG_VERSION"), "<UPSTREAM>");
    insta::assert_snapshot!(rendered, @r"
    $ codex --version
    codex-cli <UPSTREAM>
    $ codex --custom
    codex-cli <UPSTREAM>-custom.<REVISION>
    ");
    Ok(())
}

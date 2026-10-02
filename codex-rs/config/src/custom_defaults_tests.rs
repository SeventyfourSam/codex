use super::initialize_custom_config;
use pretty_assertions::assert_eq;
use std::fs;

#[test]
fn initializes_a_fresh_home_and_is_idempotent() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("new home");
    initialize_custom_config(&home).unwrap();
    let path = home.join("config.toml");
    let first = fs::read_to_string(&path).unwrap();
    assert_eq!(
        toml::from_str::<toml::Value>(&first).unwrap(),
        toml::toml! {
            check_for_update_on_startup = false
            [analytics]
            enabled = false
            [feedback]
            enabled = false
            [otel]
            exporter = "none"
            trace_exporter = "none"
            metrics_exporter = "none"
        }
        .into(),
    );
    initialize_custom_config(&home).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), first);
}

#[test]
fn preserves_explicit_settings_comments_and_inline_tables() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("config.toml");
    let original = "# Keep my choices\ncheck_for_update_on_startup = true\n\
analytics = { enabled = true } # intentional\nfeedback = {}\n\
[otel]\nmetrics_exporter = 'none' # custom\n\
[otel.exporter.otlp-http]\nendpoint = 'https://collector.invalid'\n\
[model_providers.private]\nbase_url = 'https://provider.invalid'\n";
    fs::write(&path, original).unwrap();
    initialize_custom_config(home.path()).unwrap();
    let updated = fs::read_to_string(&path).unwrap();
    let mut expected = toml::from_str::<toml::Value>(original).unwrap();
    expected["feedback"]
        .as_table_mut()
        .unwrap()
        .insert("enabled".into(), false.into());
    expected["otel"]
        .as_table_mut()
        .unwrap()
        .insert("trace_exporter".into(), "none".into());
    assert_eq!(toml::from_str::<toml::Value>(&updated).unwrap(), expected);
    for comment in ["# Keep my choices", "# intentional", "# custom"] {
        assert!(updated.contains(comment));
    }
    initialize_custom_config(home.path()).unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), updated);
}

#[test]
fn refuses_invalid_configuration_without_writing() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("config.toml");
    for invalid in ["[invalid", "analytics = false", "otel = []"] {
        fs::write(&path, invalid).unwrap();
        assert!(initialize_custom_config(home.path()).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), invalid);
    }
}

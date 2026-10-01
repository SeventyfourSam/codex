use super::*;
use pretty_assertions::assert_eq;

#[test]
fn custom_versions_compare_numeric_revisions_and_upstream_releases() {
    let versions = [
        "0.159.0-custom.1",
        "0.159.0-custom.2",
        "0.159.0-custom.10",
        "0.159.1-custom.0",
    ];
    for pair in versions.windows(2) {
        assert!(parse_custom_version(pair[0]).unwrap() < parse_custom_version(pair[1]).unwrap());
    }
    assert_eq!(
        custom_version_from_tag("v0.159.0-custom.2"),
        Some("0.159.0-custom.2")
    );
    for version in [
        "0.159.0",
        "0.159.0-custom.01",
        "0.159.0-custom.1+dev",
        "0.159.0.1-custom.2",
        "0.159-custom.1",
        "0.159.0-custom.18446744073709551616",
        "0.159.0-custom.1/../../x",
    ] {
        assert_eq!(parse_custom_version(version), None, "{version}");
    }
    assert_eq!(custom_version_from_tag("rust-v0.159.0"), None);
}

#[test]
fn latest_release_requires_complete_assets_and_a_stable_custom_tag() {
    let mut json = serde_json::json!({
        "tag_name": "v0.159.0-custom.2", "draft": false, "prerelease": false,
        "assets": [
            {"name": "codex-0.159.0-custom.2-aarch64-apple-darwin.tar.gz"},
            {"name": "codex-0.159.0-custom.2-x86_64-pc-windows-msvc.zip"},
            {"name": "SHA256SUMS"}
        ]
    });
    let release: CustomRelease = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(release.version(), Some("0.159.0-custom.2"));
    json["assets"].as_array_mut().unwrap().pop();
    assert_eq!(
        serde_json::from_value::<CustomRelease>(json.clone())
            .unwrap()
            .version(),
        None
    );
    json["draft"] = true.into();
    assert_eq!(
        serde_json::from_value::<CustomRelease>(json)
            .unwrap()
            .version(),
        None
    );
}

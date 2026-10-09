//! Additional custom package coverage without modifying upstream installer tests.

#[cfg(unix)]
#[test]
fn custom_standalone_releases_require_the_same_selection_marker() {
    let home = tempfile::tempdir().unwrap();
    let standalone = home.path().join("packages/standalone");
    let release = standalone.join("releases/0.159.0-custom.2-aarch64-apple-darwin");
    let bin = release.join("bin/codex");
    std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
    std::fs::write(&bin, "binary").unwrap();
    std::os::unix::fs::symlink(&release, standalone.join("current")).unwrap();
    let marker = standalone.join("auto-update-version");
    assert!(!super::is_stable_standalone_release(home.path(), &bin));
    std::fs::write(&marker, release.file_name().unwrap().as_encoded_bytes()).unwrap();
    assert!(super::is_stable_standalone_release(home.path(), &bin));
    std::fs::remove_file(marker).unwrap();
    assert!(!super::is_stable_standalone_release(home.path(), &bin));
}

use super::*;
use crate::legacy_core::config::ConfigBuilder;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

#[tokio::test]
async fn dismiss_version_creates_cache_file_when_missing() {
    let codex_home = tempdir().expect("temp codex home");
    let config = ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .build()
        .await
        .expect("load config");
    let version_file = version_filepath(&config);

    dismiss_version(&config, "999.0.0")
        .await
        .expect("dismiss version");

    let info = read_version_info(&version_file).expect("read version info");
    assert_eq!(info.last_checked_at, DateTime::<Utc>::UNIX_EPOCH);
    assert_eq!(
        (
            info.latest_version.as_str(),
            info.dismissed_version.as_deref()
        ),
        ("999.0.0", Some("999.0.0"))
    );
}

#[test]
fn concurrent_launches_and_failed_checks_share_the_daily_budget() {
    let home = tempdir().unwrap();
    let path = home.path().join("custom-version.json");
    let now = Utc::now();
    let attempts: [std::thread::JoinHandle<bool>; 8] = std::array::from_fn(|_| {
        let path = path.clone();
        std::thread::spawn(move || reserve_update_check(&path, now).unwrap())
    });
    let allowed = attempts
        .into_iter()
        .map(|task| task.join().unwrap())
        .filter(|allowed| *allowed)
        .count();
    assert_eq!(allowed, 1);
    assert!(!reserve_update_check(&path, now + chrono::Duration::hours(23)).unwrap());
    assert!(reserve_update_check(&path, now + chrono::Duration::hours(24)).unwrap());
    assert!(!reserve_update_check(&path, now).unwrap());
}

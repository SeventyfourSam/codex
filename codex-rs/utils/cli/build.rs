fn main() {
    println!("cargo:rerun-if-env-changed=CODEX_CUSTOM_REVISION");
    println!("cargo:rerun-if-changed=build.rs");

    let revision = std::env::var("CODEX_CUSTOM_REVISION").unwrap_or_default();
    if revision.is_empty() {
        println!("cargo:rustc-env=CODEX_CUSTOM_SUFFIX=-custom");
        return;
    }
    assert!(
        !revision.is_empty()
            && revision.bytes().all(|byte| byte.is_ascii_digit())
            && (revision == "0" || !revision.starts_with('0')),
        "CODEX_CUSTOM_REVISION must be a non-negative integer without leading zeros"
    );
    println!("cargo:rustc-env=CODEX_CUSTOM_SUFFIX=-custom.{revision}");
}

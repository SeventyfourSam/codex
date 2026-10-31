"""Run upstream snapshot tests using their development version and quota policy.

Use from the repository root, e.g.:
    python .github/scripts/run_custom_compat_tests.py -p codex-tui -E 'test(update_)'

The manifest and lockfile are restored even on failure. Do not run another Cargo
command concurrently.
"""

import os
import subprocess
import sys
from pathlib import Path

from merge_custom_upstream import replace_version

# Reuse upstream behavior tests instead of maintaining fork-specific fixtures.
# With explicit arguments this remains a general upstream compatibility runner.
UPGRADE_CHECKS = [
    [
        "--lib",
        "-p", "codex-api",
        "-p", "codex-models-manager",
        "-p", "codex-core-plugins",
        "-p", "codex-tui",
        "-E",
        "package(codex-api) | package(codex-models-manager) | "
        "(package(codex-core-plugins) & test(catalog)) | "
        "(package(codex-tui) & (test(cloud_bootstrap) | "
        "test(history_cell::tests::user_) | test(status_line_setup)))",
    ],
    [
        "-p", "codex-app-server", "--test", "all",
        "-E", "test(without_discovery_prefetch)",
    ],
]


def main() -> None:
    manifest = Path("codex-rs/Cargo.toml")
    lockfile = Path("codex-rs/Cargo.lock")
    original_manifest, original_lock = manifest.read_bytes(), lockfile.read_bytes()
    env = os.environ.copy()
    env["CODEX_CUSTOM_USAGE_POLICY"] = "upstream"
    env["CODEX_CUSTOM_PLUGIN_CATALOG_POLICY"] = "upstream"
    try:
        manifest.write_text(
            replace_version(original_manifest.decode("utf-8"), "0.0.0"),
            encoding="utf-8",
            newline="\n",
        )
        for arguments in [sys.argv[1:]] if sys.argv[1:] else UPGRADE_CHECKS:
            subprocess.run(["just", "test", *arguments], env=env, check=True)
    finally:
        manifest.write_bytes(original_manifest)
        lockfile.write_bytes(original_lock)


if __name__ == "__main__":
    main()

"""Run upstream snapshot tests using their development version and quota policy.

Use from the repository root, e.g.:
    python .github/scripts/run_custom_compat_tests.py -p codex-tui -E 'test(update_)'

The manifest and lockfile are restored even on failure. Do not run another Cargo
command concurrently. Custom tests explicitly opt their widgets into fork policy.
"""

import os
import subprocess
import sys
from pathlib import Path

from merge_custom_upstream import replace_version


def main() -> None:
    manifest = Path("codex-rs/Cargo.toml")
    lockfile = Path("codex-rs/Cargo.lock")
    original_manifest, original_lock = manifest.read_bytes(), lockfile.read_bytes()
    env = os.environ.copy()
    env["CODEX_CUSTOM_USAGE_POLICY"] = "upstream"
    try:
        manifest.write_text(
            replace_version(original_manifest.decode("utf-8"), "0.0.0"),
            encoding="utf-8",
            newline="\n",
        )
        result = subprocess.run(["just", "test", *sys.argv[1:]], env=env, check=False)
    finally:
        manifest.write_bytes(original_manifest)
        lockfile.write_bytes(original_lock)
    raise SystemExit(result.returncode)


if __name__ == "__main__":
    main()

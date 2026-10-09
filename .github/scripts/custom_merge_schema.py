"""Regenerate protocol artifacts only after a release's source merge is clean."""

import subprocess
from pathlib import Path

GENERATED_PATHS = (
    "codex-rs/app-server-protocol/schema",
    "sdk/python/src/openai_codex/generated",
)


def regenerate_schema(upstream: str) -> None:
    schema = Path(GENERATED_PATHS[0])
    if not schema.exists():
        return
    # Rust includes the compressed bundles at compile time. Seed them (and any
    # conflicted text fixtures) from upstream, then replace them from source.
    for path in GENERATED_PATHS:
        exists = subprocess.check_output(["git", "ls-tree", upstream, "--", path])
        if exists:
            subprocess.run(
                [
                    "git",
                    "restore",
                    f"--source={upstream}",
                    "--staged",
                    "--worktree",
                    "--",
                    path,
                ],
                check=True,
            )
    lockfile = Path("codex-rs/Cargo.lock")
    lock = lockfile.read_bytes()
    try:
        subprocess.run(["just", "write-app-server-schema"], check=True)
        subprocess.run(
            ["just", "write-app-server-schema", "--experimental"], check=True
        )
        subprocess.run(["just", "test", "-p", "codex-app-server-protocol"], check=True)
        if Path("codex-rs/cli/Cargo.toml").is_file():
            # Protocol tests do not compile new TUI callers of protocol structs.
            subprocess.run(
                [
                    "cargo",
                    "check",
                    "-p",
                    "codex-cli",
                    "-p",
                    "codex-tui",
                    "--tests",
                    "--message-format",
                    "short",
                ],
                cwd="codex-rs",
                check=True,
            )
    finally:
        # Cargo may rewrite workspace package versions while generating. Keep
        # the merged lockfile; regeneration is not a dependency update.
        lockfile.write_bytes(lock)
    for path in GENERATED_PATHS:
        if Path(path).exists():
            subprocess.run(["git", "add", "-A", "--", path], check=True)

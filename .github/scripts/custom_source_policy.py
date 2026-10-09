"""Reject fork changes that repeatedly break upstream callers and tests."""

import re
import subprocess
from pathlib import Path

CONTRACTS = (
    ("codex-rs/app-server-protocol/src/protocol/v2/model.rs", "ModelListParams"),
    ("codex-rs/models-manager/src/manager.rs", "RefreshStrategy"),
    ("codex-rs/cli/src/main.rs", "MultitoolCli"),
    ("codex-rs/tui/src/update_action.rs", "UpdateAction"),
)


def upstream_file(commit: str, path: str) -> str | None:
    result = subprocess.run(
        ["git", "show", f"{commit}:{path}"], capture_output=True, check=False
    )
    if result.returncode:
        return None
    return result.stdout.decode("utf-8")


def type_body(source: str, name: str) -> str:
    match = re.search(
        rf"(?ms)^(?:pub(?:\([^)]*\))?\s+)?(?:struct|enum)\s+{name}\b[^{{]*\{{(.*?)^\}}",
        source,
    )
    if not match:
        raise ValueError(f"Cannot locate upstream type {name}")
    body = re.sub(r"(?m)^\s*//[^\n]*", "", match[1])
    return re.sub(r"\s+", "", body)


def test_names(source: str) -> set[str]:
    return set(
        re.findall(
            r"#\[(?:(?:tokio::)?test|test_case|rstest)[^\]]*\]\s*(?:#\[[^\]]*\]\s*)*(?:async\s+)?fn\s+(\w+)",
            source,
        )
    )


def verify_source_contracts(upstream: str) -> None:
    errors = []
    for path, name in CONTRACTS:
        original = upstream_file(upstream, path)
        if original is None:
            continue
        current = Path(path).read_text(encoding="utf-8") if Path(path).is_file() else ""
        if type_body(original, name) != type_body(current, name):
            errors.append(f"{path}: keep upstream {name} unchanged; use a custom type")

    changed = subprocess.check_output(
        [
            "git",
            "-c",
            "core.autocrlf=false",
            "diff",
            "--name-only",
            upstream,
            "--",
            "codex-rs",
        ],
        text=True,
        encoding="utf-8",
    ).splitlines()
    for path in changed:
        if not path.endswith(".rs"):
            continue
        original = upstream_file(upstream, path)
        if original is None:
            continue
        current = Path(path).read_text(encoding="utf-8") if Path(path).is_file() else ""
        if removed := test_names(original) - test_names(current):
            errors.append(
                f"{path}: removed or renamed upstream tests {sorted(removed)}"
            )
    if errors:
        raise ValueError("Custom source policy failed:\n" + "\n".join(errors))
    print(
        "Custom source policy: upstream request types, refresh strategies, CLI args, update actions, and tests retained",
        flush=True,
    )

"""Reject fork changes that repeatedly break upstream callers and tests."""

import re
import subprocess
from difflib import SequenceMatcher
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


def blank_line_only_module_changes(original: str, current: str) -> list[int]:
    """Catch formatting-only edits in module indexes, without policing string contents."""
    before, after = original.splitlines(), current.splitlines()
    lines = []
    for operation, start_before, end_before, start_after, end_after in SequenceMatcher(
        None, before, after
    ).get_opcodes():
        changes = before[start_before:end_before] + after[start_after:end_after]
        if operation == "equal" or any(line.strip() for line in changes):
            continue
        previous = next((line for line in reversed(before[:start_before]) if line.strip()), "")
        following = next((line for line in before[end_before:] if line.strip()), "")
        if re.fullmatch(r"\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+;\s*", previous) and (
            re.match(r"\s*(?:mod\s|#\[path\s*=)", following) or not following
        ):
            lines.append(start_after + 1)
    return lines


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
        if lines := blank_line_only_module_changes(original, current):
            errors.append(f"{path}: remove blank-line-only changes near lines {lines}")
    if errors:
        raise ValueError("Custom source policy failed:\n" + "\n".join(errors))
    print(
        "Custom source policy: upstream request types, refresh strategies, CLI args, update actions, and tests retained",
        flush=True,
    )

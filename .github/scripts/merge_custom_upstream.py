#!/usr/bin/env python3
"""Merge an official stable tag, resolving only workspace-version conflicts."""

import argparse
import os
import re
import subprocess
import tempfile
from pathlib import Path

import tomllib

MANIFEST = "codex-rs/Cargo.toml"
VERSION = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")


def git(*args: str) -> str:
    return subprocess.check_output(["git", *args], text=True, encoding="utf-8")


def replace_version(manifest: str, version: str) -> str:
    old = tomllib.loads(manifest)["workspace"]["package"]["version"]
    pattern = r'(?m)(^\[workspace\.package\][^\n]*\n(?:(?!\[)[^\n]*\n)*?version\s*=\s*)"[^"\n]*"'
    result, count = re.subn(pattern, lambda match: f'{match[1]}"{version}"', manifest)
    if count != 1:
        raise ValueError("Cannot locate the workspace package version")
    expected = tomllib.loads(manifest)
    expected["workspace"]["package"]["version"] = version
    if tomllib.loads(result) != expected:
        raise ValueError(f"Replacing version {old} changed other manifest values")
    return result


def resolve_version_conflict(version: str) -> None:
    # Normalize only the version in all three index stages, then let Git merge
    # the rest. Dependency conflicts must still fail instead of choosing a side.
    with tempfile.TemporaryDirectory(prefix="codex-version-merge-") as directory:
        paths = []
        for stage in (2, 1, 3):
            path = Path(directory) / str(stage)
            path.write_text(
                replace_version(git("show", f":{stage}:{MANIFEST}"), version),
                encoding="utf-8",
                newline="\n",
            )
            paths.append(str(path))
        result = subprocess.run(
            ["git", "merge-file", "-p", *paths], capture_output=True, check=False
        )
        if result.returncode != 0:
            raise ValueError("Cargo.toml has conflicts beyond the workspace version")
        Path(MANIFEST).write_bytes(result.stdout)
    git("add", "--", MANIFEST)


def merge_release(version: str) -> None:
    if VERSION.fullmatch(version) is None:
        raise ValueError("Expected a stable version such as 0.159.2")
    if git("branch", "--show-current").strip() != "custom":
        raise ValueError("Run this command on the custom branch")
    if git("status", "--porcelain").strip():
        raise ValueError("The working tree must be clean before merging")
    if (
        subprocess.run(
            ["git", "rev-parse", "--verify", "-q", "MERGE_HEAD"],
            capture_output=True,
            check=False,
        ).returncode
        == 0
    ):
        raise ValueError("Finish the existing merge first")

    tag = f"rust-v{version}"
    # Fetch exactly the official tag, without trusting or replacing local tags.
    git("fetch", "--no-tags", "https://github.com/openai/codex.git", f"refs/tags/{tag}")
    upstream = git("rev-parse", "FETCH_HEAD^{commit}").strip()
    incoming = tomllib.loads(git("show", f"{upstream}:{MANIFEST}"))
    if incoming["workspace"]["package"]["version"] != version:
        raise ValueError("Official tag and workspace version do not match")
    if (
        subprocess.run(
            ["git", "merge-base", "--is-ancestor", upstream, "HEAD"], check=False
        ).returncode
        == 0
    ):
        print(f"{tag} is already merged")
        return
    current = tomllib.loads(Path(MANIFEST).read_text(encoding="utf-8"))["workspace"][
        "package"
    ]["version"]
    if VERSION.fullmatch(current) is None or tuple(
        map(int, version.split("."))
    ) <= tuple(map(int, current.split("."))):
        raise ValueError(
            f"Target {version} must be newer than current version {current}"
        )

    merge = subprocess.run(
        [
            "git",
            "-c",
            "rerere.enabled=false",
            "merge",
            "--no-ff",
            "--no-commit",
            upstream,
        ],
        check=False,
    )
    if merge.returncode != 0:
        conflicts = git("diff", "--name-only", "--diff-filter=U").splitlines()
        if MANIFEST in conflicts:
            resolve_version_conflict(version)
            conflicts = git("diff", "--name-only", "--diff-filter=U").splitlines()
        if conflicts:
            raise ValueError(f"Merge requires manual resolution: {conflicts}")
        git("rev-parse", "--verify", "MERGE_HEAD")
    if (
        tomllib.loads(Path(MANIFEST).read_text(encoding="utf-8"))["workspace"][
            "package"
        ]["version"]
        != version
    ):
        raise ValueError(
            "Merged workspace version does not match the requested release"
        )
    git("diff", "--cached", "--check")
    git("commit", "-m", f"Merge upstream {version} into custom")
    if summary := os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(summary, "a", encoding="utf-8") as output:
            output.write(
                f"Prepared merge of `openai/codex` tag `{tag}` into `custom`.\n\n"
                f"Commit: `{git('rev-parse', 'HEAD').strip()}`\n\n"
                "### Incoming commits\n\n```text\n"
                + git("log", "--oneline", "HEAD^1..HEAD^2")
                + "```\n"
            )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version", help="Official stable version, for example 0.159.2")
    merge_release(parser.parse_args().version)


if __name__ == "__main__":
    main()

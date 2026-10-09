#!/usr/bin/env python3
"""Upgrade custom using the previous official release as the three-way base."""

import argparse
import json
import os
import re
import subprocess
import tempfile
from pathlib import Path

import tomllib
from custom_merge_schema import GENERATED_PATHS, regenerate_schema
from custom_source_policy import verify_source_contracts

MANIFEST = "codex-rs/Cargo.toml"
BASELINE = Path(".github/custom-upstream-base.json")
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


def fetch_release(version: str) -> str:
    # Do not trust or replace a same-named local tag.
    git(
        "fetch",
        "--no-tags",
        "https://github.com/openai/codex.git",
        f"refs/tags/rust-v{version}",
    )
    commit = git("rev-parse", "FETCH_HEAD^{commit}").strip()
    if (
        tomllib.loads(git("show", f"{commit}:{MANIFEST}"))["workspace"]["package"][
            "version"
        ]
        != version
    ):
        raise ValueError("Official tag and workspace version do not match")
    return commit


def prepare_merge(base: str, upstream: str, version: str) -> None:
    # merge-tree computes the result without touching HEAD, the index or files.
    result = subprocess.run(
        [
            "git",
            "merge-tree",
            "--write-tree",
            "-z",
            f"--merge-base={base}",
            "HEAD",
            upstream,
        ],
        capture_output=True,
        check=False,
    )
    if result.returncode not in (0, 1):
        raise RuntimeError(result.stderr.decode("utf-8", errors="replace"))
    records = result.stdout.split(b"\0")
    tree = records.pop(0).decode("ascii")
    stages = []
    for record in records:
        if not record:
            break
        stages.append(record)
    git("update-ref", "ORIG_HEAD", "HEAD")
    git("read-tree", "--reset", "-u", tree)
    # Recreate unmerged index entries so normal Git conflict inspection and
    # `git merge --abort` work, including filenames containing whitespace.
    if stages:
        paths = sorted({entry.split(b"\t", 1)[1] for entry in stages})
        removals = [b"0 " + b"0" * 40 + b"\t" + path for path in paths]
        subprocess.run(
            ["git", "update-index", "-z", "--index-info"],
            input=b"\0".join(removals + stages) + b"\0",
            check=True,
        )
    for name, content in {
        "MERGE_HEAD": upstream + "\n",
        "MERGE_MSG": f"Merge upstream {version} into custom\n",
        "MERGE_MODE": "no-ff",
    }.items():
        Path(git("rev-parse", "--git-path", name).strip()).write_text(
            content, encoding="utf-8", newline="\n"
        )


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
    upstream = fetch_release(version)
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

    base = fetch_release(current)
    if BASELINE.exists() and json.loads(BASELINE.read_text(encoding="utf-8")) != {
        "version": current,
        "commit": base,
    }:
        raise ValueError(
            "Recorded upstream baseline does not match the official release"
        )
    if subprocess.run(
        ["git", "merge-base", "--is-ancestor", base, "HEAD"], check=False
    ).returncode:
        raise ValueError(
            "Previous official release is not an ancestor of custom; inspect the baseline"
        )
    print(
        f"Upgrade base: rust-v{current} ({base}); target: {tag} ({upstream})",
        flush=True,
    )
    prepare_merge(base, upstream, version)
    conflicts = git("diff", "--name-only", "--diff-filter=U").splitlines()
    if MANIFEST in conflicts:
        resolve_version_conflict(version)
        conflicts = git("diff", "--name-only", "--diff-filter=U").splitlines()
    source_conflicts = [
        path
        for path in conflicts
        if not any(
            path == generated or path.startswith(generated + "/")
            for generated in GENERATED_PATHS
        )
    ]
    if source_conflicts:
        message = f"Merge requires manual resolution: {source_conflicts}"
        print(message, flush=True)
        if summary := os.environ.get("GITHUB_STEP_SUMMARY"):
            with open(summary, "a", encoding="utf-8") as output:
                output.write(
                    f"### Source conflicts (base: rust-v{current}, target: {tag})\n\n"
                )
                output.write(
                    "\n".join(f"- `{path}`" for path in source_conflicts) + "\n\n"
                )
                output.write(
                    "Protocol artifacts will be regenerated after source conflicts are resolved.\n"
                )
        raise ValueError(message)
    if (
        tomllib.loads(Path(MANIFEST).read_text(encoding="utf-8"))["workspace"][
            "package"
        ]["version"]
        != version
    ):
        raise ValueError(
            "Merged workspace version does not match the requested release"
        )
    verify_source_contracts(upstream)
    regenerate_schema(upstream)
    BASELINE.parent.mkdir(parents=True, exist_ok=True)
    BASELINE.write_text(
        json.dumps({"version": version, "commit": upstream}, indent=2) + "\n",
        encoding="utf-8",
    )
    git("add", "--", str(BASELINE))
    # Preserve upstream whitespace (including padded UI snapshots), but still
    # reject leftover conflict markers. Inherit stdout so failures are visible.
    subprocess.run(
        [
            "git",
            "-c",
            "core.whitespace=-blank-at-eol,-blank-at-eof,-space-before-tab",
            "diff",
            "--cached",
            "--check",
        ],
        check=True,
    )
    git("commit", "-m", f"Merge upstream {version} into custom")
    if summary := os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(summary, "a", encoding="utf-8") as output:
            output.write(
                f"Prepared merge of `openai/codex` tag `{tag}` into `custom`.\n\n"
                f"Explicit upgrade base: `rust-v{current}` (`{base}`).\n\n"
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

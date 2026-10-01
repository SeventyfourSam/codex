#!/usr/bin/env python3
"""Carry the custom revision forward when OpenAI publishes a stable release."""

import json
import os
import re
import subprocess
from pathlib import Path
from urllib.error import HTTPError
from urllib.request import Request, urlopen

import tomllib
from merge_custom_upstream import MANIFEST, VERSION, git, merge_release


def github_api(path: str) -> dict | None:
    headers = {
        "Accept": "application/vnd.github+json",
        "User-Agent": "codex-custom-sync",
    }
    if token := os.environ.get("GH_TOKEN"):
        headers["Authorization"] = f"Bearer {token}"
    try:
        with urlopen(
            Request(f"https://api.github.com/{path}", headers=headers), timeout=30
        ) as response:
            return json.load(response)
    except HTTPError as error:
        if error.code == 404:
            return None
        raise


def version_tuple(version: str) -> tuple[int, ...]:
    if VERSION.fullmatch(version) is None:
        raise ValueError(f"Expected a stable version, got {version}")
    return tuple(map(int, version.split(".")))


def latest_custom_tag(tags: list[str]) -> tuple[str, int]:
    releases = []
    for tag in tags:
        if match := re.fullmatch(
            r"v([0-9]+\.[0-9]+\.[0-9]+)-custom\.(0|[1-9][0-9]*)", tag
        ):
            releases.append((version_tuple(match[1]), int(match[2]), match[1]))
    if not releases:
        raise ValueError(
            "Publish an initial custom tag before enabling automatic upgrades"
        )
    _, revision, version = max(releases)
    return version, revision


def sync_release(repository: str) -> str:
    if (
        git("branch", "--show-current").strip() != "custom"
        or git("status", "--porcelain").strip()
    ):
        raise ValueError("Start from a clean custom branch")
    upstream = github_api("repos/openai/codex/releases/latest")
    if not upstream or upstream["draft"] or upstream["prerelease"]:
        raise ValueError("No published official stable release found")
    if not upstream["tag_name"].startswith("rust-v"):
        raise ValueError("Latest official release is not a Rust CLI release")
    version = upstream["tag_name"].removeprefix("rust-v")
    target = version_tuple(version)
    current = version_tuple(
        tomllib.loads(Path(MANIFEST).read_text(encoding="utf-8"))["workspace"][
            "package"
        ]["version"]
    )
    if target < current:
        return ""
    tags = git("tag", "--merged", "HEAD", "--list", "v*-custom.*").splitlines()
    # Squashing custom history detaches the earlier release tags. Retain the
    # last published revision until a newer tag is reachable from this branch.
    baseline = Path(".github/custom-release-baseline")
    if baseline.is_file():
        tags.append(baseline.read_text(encoding="utf-8").strip())
    previous, revision = latest_custom_tag(tags)
    tag = f"v{version}-custom.{revision}"
    if target == current:
        # Retry an existing tag's incomplete build, never tag unpublished custom
        # commits just because the stable-release poll ran again.
        if previous != version:
            return ""
        release = github_api(f"repos/{repository}/releases/tags/{tag}")
        required = {
            f"codex-{tag[1:]}-aarch64-apple-darwin.tar.gz",
            f"codex-{tag[1:]}-x86_64-pc-windows-msvc.zip",
            "install.sh",
            "install.ps1",
            "SHA256SUMS",
        }
        if (
            release
            and not release["draft"]
            and not release["prerelease"]
            and required <= {asset["name"] for asset in release["assets"]}
        ):
            return ""
        return tag

    if (
        subprocess.run(
            ["git", "rev-parse", "--verify", "-q", f"refs/tags/{tag}"],
            capture_output=True,
            check=False,
        ).returncode
        == 0
    ):
        raise ValueError(
            f"Tag {tag} already exists outside custom history; inspect it manually"
        )
    merge_release(version)
    git(
        "tag",
        "-a",
        tag,
        "-m",
        f"Merge official {version}; retain custom revision {revision}",
    )
    # Either both refs update or neither does. A concurrent custom push fails
    # normally and is picked up from a fresh checkout on the next poll.
    git("push", "--atomic", "origin", "HEAD:refs/heads/custom", f"refs/tags/{tag}")
    return tag


def main() -> None:
    tag = sync_release(os.environ["GITHUB_REPOSITORY"])
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        output.write(f"release_tag={tag}\n")
    with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as output:
        output.write(
            f"Building `{tag}`.\n"
            if tag
            else "No stable upgrade or unfinished release to build.\n"
        )


if __name__ == "__main__":
    main()

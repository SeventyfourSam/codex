"""Resolve a generated Cargo.lock conflict from the merged manifests."""

import re
import subprocess
from pathlib import Path

import tomllib

CARGO_LOCK = "codex-rs/Cargo.lock"


def external_packages(lock: str) -> dict[tuple[str, str, str, str | None], str]:
    packages = {}
    for section in re.split(r"(?m)(?=^\[\[)", lock):
        if section.startswith("[[package]]"):
            package = tomllib.loads(section)["package"][0]
            if source := package.get("source"):
                pin = (package["name"], package["version"], source, package.get("checksum"))
                packages[pin] = section
    return packages


def resolve_lock_conflict(base_lock: str, custom_lock: str, upstream_lock: str) -> None:
    lock = Path(CARGO_LOCK)
    original = lock.read_bytes()
    upstream_pins = external_packages(upstream_lock)
    base_pins = external_packages(base_lock)
    custom_pins = {
        pin: section
        for pin, section in external_packages(custom_lock).items()
        if pin not in base_pins and pin not in upstream_pins
    }
    # Seed Cargo with both sets of reviewed pins. Rebuild package dependency
    # lists from the manifest, while keeping upstream's external versions.
    seed = "\n\n".join([upstream_lock.rstrip(), *(s.rstrip() for s in custom_pins.values())])
    try:
        lock.write_text(seed + "\n", encoding="utf-8", newline="\n")
        subprocess.run(["cargo", "update", "--workspace"], cwd=lock.parent, check=True)
        resolved = external_packages(lock.read_text(encoding="utf-8")).keys()
        removed = upstream_pins.keys() - resolved
        added = resolved - (upstream_pins.keys() | custom_pins.keys())
        if removed or added:
            raise ValueError(
                "Cargo.lock resolution changed external pins; review manually. "
                f"Removed: {sorted(removed, key=str)}; added: {sorted(added, key=str)}"
            )
    except Exception:
        lock.write_bytes(original)
        raise
    subprocess.run(["git", "add", "--", CARGO_LOCK], check=True)
    print("Resolved Cargo.lock from merged manifests with reviewed dependency pins", flush=True)

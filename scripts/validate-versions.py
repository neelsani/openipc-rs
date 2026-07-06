#!/usr/bin/env python3
"""Validate lockstep release metadata across the workspace."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import sys
import tomllib


ROOT = Path(__file__).resolve().parent.parent

CARGO_MANIFESTS = (
    "apps/nebulus/Cargo.toml",
    "apps/openipc-cli/Cargo.toml",
    "apps/wfb-rs/Cargo.toml",
    "crates/openipc-core/Cargo.toml",
    "crates/openipc-rtl88xx/Cargo.toml",
    "crates/openipc-uplink/Cargo.toml",
    "crates/openipc-video/Cargo.toml",
    "crates/openipc-web/Cargo.toml",
    "apps/openipc-station/src-tauri/Cargo.toml",
    "plugins/tauri-plugin-openipc-usb/Cargo.toml",
)

JSON_PACKAGES = (
    "crates/openipc-web/package.json",
    "apps/openipc-station/package.json",
    "docs/package.json",
)

BUN_LOCKS = (
    "apps/openipc-station/bun.lock",
    "docs/bun.lock",
)

IGNORED_DIRECTORIES = {
    ".docusaurus",
    ".git",
    ".trunk",
    "build",
    "dist",
    "node_modules",
    "pkg",
    "target",
}


def package_lock_files() -> list[Path]:
    locks: list[Path] = []
    for current, directories, files in os.walk(ROOT):
        directories[:] = [
            directory
            for directory in directories
            if directory not in IGNORED_DIRECTORIES
        ]
        if "package-lock.json" in files:
            locks.append(Path(current, "package-lock.json").relative_to(ROOT))
    return sorted(locks)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--tag",
        help="also require this v-prefixed Git tag to match the workspace version",
    )
    args = parser.parse_args()

    expected = tomllib.loads(
        (ROOT / "crates/openipc-core/Cargo.toml").read_text()
    )["package"]["version"]
    errors: list[str] = []

    for relative in CARGO_MANIFESTS:
        version = tomllib.loads((ROOT / relative).read_text())["package"]["version"]
        if version != expected:
            errors.append(f"{relative}: {version} != {expected}")

    for relative in JSON_PACKAGES:
        version = json.loads((ROOT / relative).read_text())["version"]
        if version != expected:
            errors.append(f"{relative}: {version} != {expected}")

    for relative in BUN_LOCKS:
        path = ROOT / relative
        if not path.is_file():
            errors.append(f"{relative} is missing")
            continue
        text = path.read_text()
        if any(marker in text for marker in ("/api/npm/", "artifacthub", "oraclecorp")):
            errors.append(f"{relative} contains a non-public registry URL")

    locks = package_lock_files()
    if locks:
        errors.append(
            "package-lock.json files are not used; found: "
            + ", ".join(str(path) for path in locks)
        )

    changelog = (ROOT / "CHANGELOG.md").read_text()
    if f"## {expected} " not in changelog and f"## {expected}\n" not in changelog:
        errors.append(f"CHANGELOG.md does not contain a {expected} section")

    if args.tag:
        if not args.tag.startswith("v"):
            errors.append(f"release tag must start with v: {args.tag}")
        elif args.tag[1:] != expected:
            errors.append(f"release tag {args.tag} does not match workspace {expected}")

    if errors:
        print("Version metadata is out of sync:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1

    suffix = f" and tag {args.tag}" if args.tag else ""
    print(f"All release metadata is at {expected}{suffix}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Check the release selected by a flake and, optionally, its installed tools."""

import argparse
import re
import subprocess
from pathlib import Path


def cargo_version(root, cargo=None):
    if cargo is None:
        cargo = (root / "Cargo.toml").read_text()
    workspace = re.search(r"(?ms)^\[workspace\.package\]\s*\n(.*?)(?=^\[|\Z)", cargo)
    if workspace is None:
        raise ValueError("Cargo.toml has no workspace.package section")
    return field(workspace[1], "version")


def field(text, name):
    matches = re.findall(rf'^\s*{name}\s*=\s*"([^"]+)"\s*;?\s*$', text, re.M)
    if len(matches) != 1:
        raise ValueError(f"expected one {name} declaration")
    return matches[0]


def validate(root, version=None, release_tag=None, package=None, base_ref=None):
    expected = version or cargo_version(root)
    if not re.fullmatch(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", expected):
        raise ValueError("expected a stable X.Y.Z version")
    tag = release_tag or f"v{expected}"
    if tag not in (f"v{expected}", "snapshot"):
        raise ValueError("release tag does not match the expected version")
    flake = (root / "flake.nix").read_text()
    if field(flake, "releaseVersion") != expected or field(flake, "releaseTag") != tag:
        raise ValueError(f"flake must select {tag} at version {expected}")
    if base_ref:
        cargo = subprocess.check_output(["git", "-C", str(root), "show", f"{base_ref}:Cargo.toml"],
                                        text=True, stderr=subprocess.DEVNULL)
        base = cargo_version(root, cargo)
        if tuple(map(int, expected.split("."))) < tuple(map(int, base.split("."))):
            raise ValueError("candidate release is older than the main base")
    if package:
        for name in ("gpclient", "gpservice", "gpauth", "gpgui"):
            result = subprocess.run([str(package / "bin" / name), "--version"],
                                    capture_output=True, text=True, check=True)
            # Keep private source identity and other command output out of logs.
            match = re.match(rf"{name}\s+(\d+\.\d+\.\d+)(?:\s|$)", result.stdout)
            if match is None or match[1] != expected:
                raise ValueError(f"installed {name} does not report version {expected}")
    return expected


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--version", help="Explicit candidate version; defaults to Cargo.toml")
    parser.add_argument("--release-tag", help="Explicit candidate tag; defaults to v<version>")
    parser.add_argument("--package", type=Path)
    parser.add_argument("--base-ref", help="Reject a release older than this main commit")
    args = parser.parse_args()
    try:
        version = validate(args.root, args.version, args.release_tag, args.package, args.base_ref)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        # CalledProcessError does not include captured stdout/stderr in its string.
        parser.exit(1, f"Release version check failed: {error}\n")
    print(f"Verified release version {version}")


if __name__ == "__main__":
    main()

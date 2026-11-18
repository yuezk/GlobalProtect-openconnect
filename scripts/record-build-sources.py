#!/usr/bin/env python3
"""Retain one source and version identity per build run, including retries."""

import argparse
import json
import os
import re
import runpy
import subprocess
from pathlib import Path


def record(path, gp, gpgui, output, build_version):
    if path.exists():
        identity = json.loads(path.read_text())
    else:
        identity = {
            name: subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"],
                                          text=True, stderr=subprocess.DEVNULL).strip()
            for name, root in (("gp_sha", gp), ("gpgui_sha", gpgui))
        }
        cargo_version = runpy.run_path(str(Path(__file__).with_name("validate-release-version.py")))["cargo_version"]
        version = cargo_version(gp)
        if cargo_version(gpgui) != version:
            raise ValueError("public and desktop product versions differ")
        identity.update(version=version, build_version=build_version)
    if set(identity) != {"gp_sha", "gpgui_sha", "version", "build_version"} or any(
        not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{40}", value)
        for name, value in identity.items() if name.endswith("_sha")
    ):
        raise ValueError("invalid retained build source refs")
    if not isinstance(identity["version"], str) or not re.fullmatch(r"\d+\.\d+\.\d+", identity["version"]):
        raise ValueError("invalid retained product version")
    if not isinstance(identity["build_version"], str) or not re.fullmatch(
        r"(0|[1-9]\d*)\.(0|[1-9]\d*)", identity["build_version"]
    ):
        raise ValueError("invalid retained build version")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(identity, indent=2) + "\n")
    with output.open("a") as stream:
        for name, value in identity.items():
            stream.write(f"{name}={value}\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path", type=Path)
    parser.add_argument("--gp", type=Path, required=True)
    parser.add_argument("--gpgui", type=Path, required=True)
    parser.add_argument("--build-version", required=True)
    args = parser.parse_args()
    try:
        record(args.path, args.gp, args.gpgui, Path(os.environ["GITHUB_OUTPUT"]), args.build_version)
    except (OSError, ValueError, subprocess.CalledProcessError):
        parser.exit(1, "Could not retain the selected build source refs.\n")


if __name__ == "__main__":
    main()

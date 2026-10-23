#!/usr/bin/env python3
"""Retain one source pair per build run so retries cannot move branch refs."""

import argparse
import json
import os
import re
import subprocess
from pathlib import Path


def record(path, gp, gpgui, output):
    if path.exists():
        sources = json.loads(path.read_text())
    else:
        sources = {
            name: subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"],
                                          text=True, stderr=subprocess.DEVNULL).strip()
            for name, root in (("gp_sha", gp), ("gpgui_sha", gpgui))
        }
    if set(sources) != {"gp_sha", "gpgui_sha"} or any(
        not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{40}", value)
        for value in sources.values()
    ):
        raise ValueError("invalid retained build source refs")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(sources, indent=2) + "\n")
    with output.open("a") as stream:
        for name, sha in sources.items():
            stream.write(f"{name}={sha}\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path", type=Path)
    parser.add_argument("--gp", type=Path, required=True)
    parser.add_argument("--gpgui", type=Path, required=True)
    args = parser.parse_args()
    try:
        record(args.path, args.gp, args.gpgui, Path(os.environ["GITHUB_OUTPUT"]))
    except (OSError, ValueError, subprocess.CalledProcessError):
        parser.exit(1, "Could not retain the selected build source refs.\n")


if __name__ == "__main__":
    main()

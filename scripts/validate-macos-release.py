#!/usr/bin/env python3
"""Validate the identity and checksums of a completed public macOS build."""

import argparse
import hashlib
import json
from pathlib import Path
import re


def validate(directory, release_tag, workflow_commit):
    metadata = json.loads((directory / "release-build.json").read_text())
    for name in ("gp_ref", "gpgui_ref", "workflow_sha"):
        if not re.fullmatch(r"[0-9a-f]{40}", metadata[name]):
            raise ValueError(f"Invalid {name}")
    if metadata["workflow_sha"] != workflow_commit:
        raise ValueError("Artifact identity does not match its public workflow run")
    if metadata["release_tag"] != release_tag:
        raise ValueError("Artifact release tag does not match the requested release")
    if not re.fullmatch(r"[0-9]+\.[0-9]+", metadata["build_version"]):
        raise ValueError("Invalid build version")
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.macos", metadata["request_id"]):
        raise ValueError("Invalid build request ID")

    version = metadata["artifact_version"]
    if metadata["channel"] == "stable":
        if not re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+", release_tag):
            raise ValueError("Invalid stable tag")
        if version != release_tag[1:]:
            raise ValueError("Artifact version does not match the stable tag")
    elif metadata["channel"] == "snapshot":
        if release_tag != "snapshot" or not re.fullmatch(
            r"[0-9]+\.[0-9]+\.[0-9]+-snapshot\." + re.escape(metadata["build_version"]), version
        ):
            raise ValueError("Invalid snapshot artifact version")
    else:
        raise ValueError("Invalid channel")

    expected = {f"GPConnect_{version}_arm64.{extension}" for extension in ("dmg", "zip")}
    expected |= {name + ".sha256" for name in expected}
    if set(metadata["assets"]) != expected:
        raise ValueError("Build must contain exactly the DMG, ZIP, and their checksums")
    if {file.name for file in directory.iterdir()} != expected | {"release-build.json"}:
        raise ValueError("Unexpected or missing artifact files")
    for name, digest in metadata["assets"].items():
        file = directory / name
        if file.is_symlink() or hashlib.sha256(file.read_bytes()).hexdigest() != digest:
            raise ValueError(f"Artifact checksum mismatch: {name}")
    for name in expected:
        if name.endswith(".sha256"):
            fields = (directory / name).read_text().split()
            if fields != [metadata["assets"][name[:-7]], name[:-7]]:
                raise ValueError(f"Invalid checksum file: {name}")
    return metadata


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--release-tag", required=True)
    parser.add_argument("--workflow-commit", required=True)
    args = parser.parse_args()
    try:
        result = validate(args.directory, args.release_tag, args.workflow_commit)
    except (ValueError, KeyError, OSError) as error:
        parser.exit(1, f"{error}\n")
    print(json.dumps(result))

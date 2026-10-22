#!/usr/bin/env python3

from __future__ import annotations

import argparse
import datetime
import json
import re
import unicodedata
from pathlib import Path
from urllib.parse import urlparse

ALLOWED_PLATFORMS = {"linux", "freebsd", "openbsd"}
SEMVER = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")
BUILD = re.compile(r"^(0|[1-9][0-9]*)(?:\.(0|[1-9][0-9]*)){0,2}$")
EXPECTED_TOP_LEVEL_KEYS = {"schemaVersion", "stable", "snapshot"}
EXPECTED_STABLE_KEYS = {"version", "releasedAt", "releaseUrl", "platforms", "highlights"}
EXPECTED_SNAPSHOT_KEYS = {"baseVersion", "build", "releasedAt", "releaseUrl", "platforms", "highlights"}
MAX_U64 = 2**64 - 1


def version_tuple(version: str) -> tuple[int, int, int]:
    match = SEMVER.fullmatch(version)
    if not match:
        raise ValueError("version must be stable semantic versioning")
    parts = tuple(int(part) for part in match.groups())
    if any(part > MAX_U64 for part in parts):
        raise ValueError("version component exceeds the supported integer range")
    return parts


def build_tuple(build: str) -> tuple[int, int, int]:
    if not BUILD.fullmatch(build):
        raise ValueError("snapshot build must contain one to three numeric components")
    parts = [int(part) for part in build.split(".")]
    if any(part > MAX_U64 for part in parts):
        raise ValueError("snapshot build component exceeds the supported integer range")
    parts.extend([0] * (3 - len(parts)))
    return parts[0], parts[1], parts[2]


def validate_release_fields(release: dict) -> None:
    try:
        datetime.date.fromisoformat(release["releasedAt"])
    except (TypeError, ValueError) as error:
        raise ValueError("releasedAt must be an ISO calendar date") from error

    parsed_url = urlparse(release["releaseUrl"])
    if parsed_url.scheme != "https" or parsed_url.hostname != "github.com":
        raise ValueError("releaseUrl must use GitHub HTTPS")

    platforms = release["platforms"]
    if not isinstance(platforms, list) or not platforms:
        raise ValueError("platforms must be a non-empty list")
    if any(not isinstance(platform, str) or platform not in ALLOWED_PLATFORMS for platform in platforms):
        raise ValueError("platforms contains an unsupported target")
    if len(set(platforms)) != len(platforms):
        raise ValueError("platforms contains duplicates")

    highlights = release["highlights"]
    if not isinstance(highlights, list) or not 1 <= len(highlights) <= 6:
        raise ValueError("highlights must contain between one and six entries")
    for highlight in highlights:
        if not isinstance(highlight, str) or not highlight.strip():
            raise ValueError("highlights must be non-empty strings")
        if len(highlight) > 160:
            raise ValueError("highlight exceeds 160 characters")
        if any(unicodedata.category(character) == "Cc" for character in highlight):
            raise ValueError("highlight contains control characters")


def validate_stable(release: object, repository: str) -> dict | None:
    if release is None:
        return None
    if not isinstance(release, dict) or set(release) != EXPECTED_STABLE_KEYS:
        raise ValueError("stable release has unexpected fields")
    version = release["version"]
    if not isinstance(version, str):
        raise ValueError("stable version must be a string")
    version_tuple(version)
    expected_url = f"https://github.com/{repository}/releases/tag/v{version}"
    if release["releaseUrl"] != expected_url:
        raise ValueError(f"stable releaseUrl must be {expected_url}")
    validate_release_fields(release)
    return release


def validate_snapshot(release: object, repository: str) -> dict | None:
    if release is None:
        return None
    if not isinstance(release, dict) or set(release) != EXPECTED_SNAPSHOT_KEYS:
        raise ValueError("snapshot release has unexpected fields")
    base_version = release["baseVersion"]
    if not isinstance(base_version, str):
        raise ValueError("snapshot baseVersion must be a string")
    version_tuple(base_version)
    build = release["build"]
    if not isinstance(build, str):
        raise ValueError("snapshot build must be a string")
    build_tuple(build)
    expected_url = f"https://github.com/{repository}/releases/tag/snapshot"
    if release["releaseUrl"] != expected_url:
        raise ValueError(f"snapshot releaseUrl must be {expected_url}")
    validate_release_fields(release)
    return release


def validate_manifest(data: object, repository: str) -> dict:
    if not isinstance(data, dict) or set(data) != EXPECTED_TOP_LEVEL_KEYS:
        raise ValueError("manifest has unexpected top-level fields")
    if type(data["schemaVersion"]) is not int or data["schemaVersion"] != 1:
        raise ValueError("unsupported schemaVersion")
    stable = validate_stable(data["stable"], repository)
    snapshot = validate_snapshot(data["snapshot"], repository)
    if stable is None and snapshot is None:
        raise ValueError("manifest must contain a stable or snapshot release")
    return data


def load_highlights(path: Path) -> list[str]:
    highlights = [line.strip() for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
    if not highlights:
        raise ValueError("at least one release highlight is required")
    return highlights


def generate(args: argparse.Namespace) -> None:
    if args.input is None:
        data = {"schemaVersion": 1, "stable": None, "snapshot": None}
    else:
        data = validate_manifest(json.loads(args.input.read_text(encoding="utf-8")), args.repository)

    common = {
        "releasedAt": args.released_at,
        "platforms": args.platform if args.platform is not None else ["linux", "freebsd", "openbsd"],
        "highlights": load_highlights(args.highlights_file),
    }
    if args.channel == "stable":
        if args.build is not None:
            raise ValueError("stable releases must not specify a snapshot build")
        current = data["stable"]
        if current is not None and version_tuple(args.version) < version_tuple(current["version"]):
            raise ValueError(f"stable version {args.version} is older than {current['version']}")
        data["stable"] = {
            "version": args.version,
            "releaseUrl": f"https://github.com/{args.repository}/releases/tag/v{args.version}",
            **common,
        }
    else:
        if args.build is None:
            raise ValueError("snapshot releases require a build")
        candidate = (version_tuple(args.version), build_tuple(args.build))
        current = data["snapshot"]
        if current is not None:
            published = (version_tuple(current["baseVersion"]), build_tuple(current["build"]))
            if candidate < published:
                raise ValueError("snapshot release is older than the published snapshot")
        data["snapshot"] = {
            "baseVersion": args.version,
            "build": args.build,
            "releaseUrl": f"https://github.com/{args.repository}/releases/tag/snapshot",
            **common,
        }

    validate_manifest(data, args.repository)
    args.output.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")


def validate(args: argparse.Namespace) -> None:
    data = validate_manifest(json.loads(args.manifest.read_text(encoding="utf-8")), args.repository)
    if args.channel is None:
        return
    release = data[args.channel]
    if release is None:
        raise ValueError(f"manifest does not contain a {args.channel} release")
    version_key = "version" if args.channel == "stable" else "baseVersion"
    if args.version is not None and release[version_key] != args.version:
        raise ValueError(f"{args.channel} version {release[version_key]} does not match {args.version}")
    if args.build is not None:
        if args.channel != "snapshot":
            raise ValueError("only snapshot releases have a build")
        if release["build"] != args.build:
            raise ValueError(f"snapshot build {release['build']} does not match {args.build}")


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate and validate the GP Connect update catalog")
    subcommands = parser.add_subparsers(required=True)

    generate_parser = subcommands.add_parser("generate")
    generate_parser.add_argument("--repository", required=True)
    generate_parser.add_argument("--channel", choices=("stable", "snapshot"), required=True)
    generate_parser.add_argument("--version", required=True)
    generate_parser.add_argument("--platform", action="append", choices=sorted(ALLOWED_PLATFORMS),
                                 help="Published Unix target; repeat for multiple targets (default: all)")
    generate_parser.add_argument("--build")
    generate_parser.add_argument("--released-at", required=True)
    generate_parser.add_argument("--highlights-file", required=True, type=Path)
    generate_parser.add_argument("--input", type=Path)
    generate_parser.add_argument("--output", required=True, type=Path)
    generate_parser.set_defaults(handler=generate)

    validate_parser = subcommands.add_parser("validate")
    validate_parser.add_argument("manifest", type=Path)
    validate_parser.add_argument("--repository", required=True)
    validate_parser.add_argument("--channel", choices=("stable", "snapshot"))
    validate_parser.add_argument("--version")
    validate_parser.add_argument("--build")
    validate_parser.set_defaults(handler=validate)

    args = parser.parse_args()
    try:
        args.handler(args)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        parser.error(str(error))


if __name__ == "__main__":
    main()

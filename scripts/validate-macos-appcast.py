#!/usr/bin/env python3

import argparse
import xml.etree.ElementTree as ET
from dataclasses import dataclass
from pathlib import Path
from typing import Optional


SPARKLE_NAMESPACE = "http://www.andymatuschak.org/xml-namespaces/sparkle"
SUPPORTED_CHANNELS = {None, "snapshot"}


@dataclass(frozen=True)
class AppcastItem:
    version: str
    short_version: str
    channel: Optional[str]
    archive_url: str
    signature: str


def parse_version(value: str) -> tuple[int, int, int]:
    parts = value.split(".")
    if not 1 <= len(parts) <= 3 or any(not part.isdigit() for part in parts):
        raise ValueError(f"invalid build version: {value}")
    numbers = tuple(map(int, parts))
    return numbers + (0,) * (3 - len(numbers))


def parse_appcast(path: Path, repository: str) -> list[AppcastItem]:
    try:
        root = ET.parse(path)
    except (ET.ParseError, OSError) as error:
        raise ValueError(f"invalid appcast XML: {error}") from error

    download_prefix = f"https://github.com/{repository}/releases/download/"
    items = []
    channel_counts: dict[Optional[str], int] = {}
    versions = set()

    for element in root.findall("./channel/item"):
        version = element.findtext(f"{{{SPARKLE_NAMESPACE}}}version") or ""
        short_version = element.findtext(f"{{{SPARKLE_NAMESPACE}}}shortVersionString") or ""
        channel = element.findtext(f"{{{SPARKLE_NAMESPACE}}}channel")
        enclosure = element.find("enclosure")
        if not version or not short_version or enclosure is None:
            raise ValueError("appcast item is missing required version or enclosure metadata")
        parse_version(version)
        if channel not in SUPPORTED_CHANNELS:
            raise ValueError(f"unsupported Sparkle channel: {channel}")
        archive_url = enclosure.get("url", "")
        signature = enclosure.get(f"{{{SPARKLE_NAMESPACE}}}edSignature", "")
        length = enclosure.get("length", "")
        if not archive_url.startswith(download_prefix):
            raise ValueError(f"unexpected update archive URL: {archive_url}")
        if not signature:
            raise ValueError(f"update archive {archive_url} has no EdDSA signature")
        if not length.isdigit() or int(length) <= 0:
            raise ValueError(f"update archive {archive_url} has an invalid length")
        if version in versions:
            raise ValueError(f"duplicate build version in appcast: {version}")

        versions.add(version)
        channel_counts[channel] = channel_counts.get(channel, 0) + 1
        items.append(AppcastItem(version, short_version, channel, archive_url, signature))

    if not items:
        raise ValueError("appcast contains no update items")
    for channel, count in channel_counts.items():
        if count > 1:
            name = channel or "stable"
            raise ValueError(f"appcast contains more than one {name} update")
    return items


def command_status(args: argparse.Namespace) -> None:
    candidate = parse_version(args.build_version)
    published = max(
        parse_version(item.version)
        for item in parse_appcast(args.appcast, args.repository)
    )
    if candidate < published:
        print("older")
    elif candidate == published:
        print("same")
    else:
        print("newer")


def command_validate(args: argparse.Namespace) -> None:
    expected_channel = None if args.channel == "stable" else args.channel
    matches = [
        item
        for item in parse_appcast(args.appcast, args.repository)
        if parse_version(item.version) == parse_version(args.build_version)
    ]
    if len(matches) != 1:
        raise ValueError(f"expected one appcast item for build {args.build_version}")

    item = matches[0]
    if item.version != args.build_version:
        raise ValueError(f"appcast uses a non-canonical build version: {item.version}")
    if item.short_version != args.short_version:
        raise ValueError(f"unexpected short version: {item.short_version}")
    if item.channel != expected_channel:
        raise ValueError(f"unexpected Sparkle channel: {item.channel}")
    if item.archive_url != args.archive_url:
        raise ValueError(f"unexpected update archive URL: {item.archive_url}")


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description="Validate the GP Connect Sparkle appcast")
    subcommands = root.add_subparsers(dest="command", required=True)

    status = subcommands.add_parser("status", help="compare a candidate build with the published feed")
    status.add_argument("appcast", type=Path)
    status.add_argument("--repository", required=True)
    status.add_argument("--build-version", required=True)
    status.set_defaults(handler=command_status)

    validate = subcommands.add_parser("validate", help="validate a specific update item and the complete feed")
    validate.add_argument("appcast", type=Path)
    validate.add_argument("--repository", required=True)
    validate.add_argument("--build-version", required=True)
    validate.add_argument("--short-version", required=True)
    validate.add_argument("--channel", choices=("stable", "snapshot"), required=True)
    validate.add_argument("--archive-url", required=True)
    validate.set_defaults(handler=command_validate)
    return root


def main() -> None:
    args = parser().parse_args()
    try:
        args.handler(args)
    except ValueError as error:
        raise SystemExit(str(error)) from error


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Build one Windows catalog and appcast from verified native installers."""
import argparse
import base64
from dataclasses import dataclass
import hashlib
import importlib.util
import json
from pathlib import Path
from urllib.parse import urlparse
import xml.etree.ElementTree as ET

SPARKLE = "http://www.andymatuschak.org/xml-namespaces/sparkle"
ET.register_namespace("sparkle", SPARKLE)
spec = importlib.util.spec_from_file_location("update_manifest", Path(__file__).with_name("validate-update-manifest.py"))
manifest = importlib.util.module_from_spec(spec)
spec.loader.exec_module(manifest)


@dataclass
class Payload:
    architecture: str
    installer: Path
    installer_url: str
    signature_file: Path


def release_version(version: str, channel: str, build: str | None) -> str:
    if channel == "snapshot":
        return f"{version}-snapshot.{build}"
    return version


def installer_name(version: str, channel: str, build: str | None, architecture: str = "x64") -> str:
    return f"GPConnect_{release_version(version, channel, build)}_windows-{architecture}-setup.exe"


def validate_payload(payload: Payload, args: argparse.Namespace) -> tuple[str, int]:
    if payload.architecture not in {"x64", "arm64"}:
        raise ValueError("unsupported Windows architecture")
    expected_name = installer_name(args.version, args.channel, args.build, payload.architecture)
    if not args.allow_localhost and payload.installer.name != expected_name:
        raise ValueError("installer filename does not match the version, channel, build and architecture")
    parsed = urlparse(payload.installer_url)
    tag = f"v{args.version}" if args.channel == "stable" else "snapshot"
    expected = f"https://github.com/{args.repository}/releases/download/{tag}/"
    local = args.allow_localhost and parsed.scheme == "http" and parsed.hostname in {"localhost", "127.0.0.1", "::1"}
    if not local and not payload.installer_url.startswith(expected):
        raise ValueError("installer URL must identify the matching GitHub release asset")
    if parsed.query or parsed.fragment or not parsed.path.endswith(".exe"):
        raise ValueError("installer URL must name an EXE without query or fragment")
    if not local and parsed.path.rsplit("/", 1)[-1] != expected_name:
        raise ValueError("installer download filename does not match the version, channel, build and architecture")
    signature = payload.signature_file.read_text(encoding="ascii").strip()
    if len(base64.b64decode(signature, validate=True)) != 64:
        raise ValueError("EdDSA signature must contain 64 bytes")
    length = payload.installer.stat().st_size
    if length == 0:
        raise ValueError("installer is empty")
    return signature, length


def generate(args: argparse.Namespace) -> None:
    manifest.version_tuple(args.version)
    if args.channel == "snapshot":
        if args.build is None:
            raise ValueError("snapshot releases require --build")
        manifest.build_tuple(args.build)
    elif args.build is not None:
        raise ValueError("stable releases must not specify --build")
    if not args.payload or len({payload.architecture for payload in args.payload}) != len(args.payload):
        raise ValueError("provide one installer per Windows architecture")
    prepared = [(payload, *validate_payload(payload, args)) for payload in args.payload]
    if args.channel == "snapshot" and args.input is not None:
        previous = manifest.validate_manifest(json.loads(args.input.read_text(encoding="utf-8")), args.repository)
        stable = previous["stable"]
        if stable is not None and manifest.version_tuple(args.version) < manifest.version_tuple(stable["version"]):
            raise ValueError("snapshot base version is older than the published stable release")
    output = args.output
    output.mkdir(parents=True, exist_ok=True)
    catalog = output / "latest.json"
    manifest.generate(argparse.Namespace(
        repository=args.repository, channel=args.channel, version=args.version,
        platform=["windows"], build=args.build, released_at=args.released_at,
        highlights_file=args.highlights_file, input=args.input, output=catalog,
    ))
    data = json.loads(catalog.read_text(encoding="utf-8"))
    if any(release and release["platforms"] != ["windows"] for release in [data["stable"], data["snapshot"]]):
        raise ValueError("Windows catalog may contain only the Windows target")
    release = data[args.channel]
    display_version = release_version(args.version, args.channel, args.build)
    rss = ET.Element("rss", {"version": "2.0"})
    channel = ET.SubElement(rss, "channel")
    ET.SubElement(channel, "title").text = "GP Connect for Windows"
    for payload, signature, length in prepared:
        item = ET.SubElement(channel, "item")
        ET.SubElement(item, "title").text = f"GP Connect {display_version}"
        ET.SubElement(item, "description").text = "\n".join(release["highlights"])
        minimum = "10.0.22000" if payload.architecture == "arm64" else "10.0.19041"
        ET.SubElement(item, f"{{{SPARKLE}}}minimumSystemVersion").text = minimum
        ET.SubElement(item, "enclosure", {
            "url": payload.installer_url, "length": str(length), "type": "application/octet-stream",
            f"{{{SPARKLE}}}version": display_version,
            f"{{{SPARKLE}}}shortVersionString": display_version,
            f"{{{SPARKLE}}}installerArguments": "/UPDATE",
            f"{{{SPARKLE}}}os": f"windows-{payload.architecture}", f"{{{SPARKLE}}}edSignature": signature,
        })
    ET.indent(rss)
    channels = [args.channel]
    snapshot = data["snapshot"]
    if args.channel == "stable" and (
        snapshot is None or manifest.version_tuple(args.version) > manifest.version_tuple(snapshot["baseVersion"])
    ):
        # A newer stable release also updates snapshot installations.
        channels.append("snapshot")
    for target_channel in channels:
        channel_directory = output / target_channel
        channel_directory.mkdir(exist_ok=True)
        ET.ElementTree(rss).write(channel_directory / "appcast.xml", encoding="utf-8", xml_declaration=True)
    for payload, _, _ in prepared:
        with payload.installer.open("rb") as installer:
            digest = hashlib.file_digest(installer, "sha256").hexdigest()
        (output / (payload.installer.name + ".sha256")).write_text(f"{digest}  {payload.installer.name}\n", encoding="ascii")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--payload", nargs=4, action="append", required=True,
                        metavar=("ARCH", "INSTALLER", "URL", "SIGNATURE_FILE"))
    parser.add_argument("--repository", default="yuezk/GlobalProtect-openconnect")
    parser.add_argument("--channel", choices=("stable", "snapshot"), required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--build")
    parser.add_argument("--released-at", required=True)
    parser.add_argument("--highlights-file", type=Path, required=True)
    parser.add_argument("--input", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--allow-localhost", action="store_true", help="Permit an isolated loopback test download")
    args = parser.parse_args()
    args.payload = [Payload(architecture, Path(installer), url, Path(signature))
                    for architecture, installer, url, signature in args.payload]
    try:
        generate(args)
    except (OSError, ValueError) as error:
        parser.error(str(error))


if __name__ == "__main__":
    main()

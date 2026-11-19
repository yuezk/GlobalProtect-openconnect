import argparse
import base64
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import HTTPError
import xml.etree.ElementTree as ET


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).parents[1] / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


feed = load("windows_update_feed", "windows-update-feed.py")
preserver = load("preserve_update_feeds", "preserve-update-feeds.py")


class WindowsFeedTests(unittest.TestCase):
    def args(self, root, channel="stable", version="3.4.0", build=None, architecture="x64"):
        name = feed.installer_name(version, channel, build, architecture)
        installer = root / name
        installer.write_bytes(f"test installer {architecture}".encode())
        signature = root / f"installer-{architecture}.sig"
        signature.write_text(base64.b64encode(bytes([1 if architecture == "arm64" else 0]) * 64).decode())
        highlights = root / "highlights.txt"
        highlights.write_text("Update & improve <Windows>.\n")
        tag = f"v{version}" if channel == "stable" else "snapshot"
        return argparse.Namespace(payload=[feed.Payload(architecture, installer,
            f"https://github.com/yuezk/GlobalProtect-openconnect/releases/download/{tag}/{name}", signature)],
            repository="yuezk/GlobalProtect-openconnect", channel=channel,
            version=version, build=build, released_at="2026-11-14",
            highlights_file=highlights, input=None, output=root / "windows", allow_localhost=False)

    def test_catalog_and_appcast_keep_the_same_version_and_payload(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.args(Path(directory))
            feed.generate(args)
            catalog = json.loads((args.output / "latest.json").read_text())
            self.assertEqual(catalog["stable"]["platforms"], ["windows"])
            item = ET.parse(args.output / "stable/appcast.xml").find("channel/item")
            self.assertEqual(item.findtext("description"), "Update & improve <Windows>.")
            enclosure = item.find("enclosure")
            self.assertEqual(enclosure.get(f"{{{feed.SPARKLE}}}version"), catalog["stable"]["version"])
            self.assertEqual(int(enclosure.get("length")), args.payload[0].installer.stat().st_size)
            self.assertEqual(enclosure.get(f"{{{feed.SPARKLE}}}os"), "windows-x64")

    def test_native_architectures_share_one_release_with_separate_signed_enclosures(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.args(Path(directory))
            arm = self.args(Path(directory), architecture="arm64")
            args.payload += arm.payload
            feed.generate(args)
            items = ET.parse(args.output / "stable/appcast.xml").findall("channel/item")
            self.assertEqual(len(items), 2)
            for item, payload in zip(items, args.payload):
                enclosure = item.find("enclosure")
                self.assertEqual(enclosure.get("url"), payload.installer_url)
                self.assertEqual(int(enclosure.get("length")), payload.installer.stat().st_size)
                self.assertEqual(enclosure.get(f"{{{feed.SPARKLE}}}edSignature"), payload.signature_file.read_text())
                self.assertEqual(enclosure.get(f"{{{feed.SPARKLE}}}os"), f"windows-{payload.architecture}")
                self.assertEqual(enclosure.get(f"{{{feed.SPARKLE}}}installerArguments"), "/UPDATE")
                self.assertEqual(enclosure.get(f"{{{feed.SPARKLE}}}version"), "3.4.0")
                minimum = "10.0.22000" if payload.architecture == "arm64" else "10.0.17763"
                self.assertEqual(item.findtext(f"{{{feed.SPARKLE}}}minimumSystemVersion"), minimum)
            self.assertEqual(json.loads((args.output / "latest.json").read_text())["stable"]["platforms"], ["windows"])

    def test_mismatched_or_duplicate_architecture_does_not_write_a_feed(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.args(Path(directory))
            args.payload[0].architecture = "arm64"
            with self.assertRaisesRegex(ValueError, "filename"):
                feed.generate(args)
            self.assertFalse(args.output.exists())
            args.payload[0].architecture = "x64"
            args.payload *= 2
            with self.assertRaisesRegex(ValueError, "one installer per"):
                feed.generate(args)
            self.assertFalse(args.output.exists())

    def test_snapshot_promotion_retains_stable_and_rejects_downgrade(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.args(Path(directory))
            feed.generate(args)
            args.input = args.output / "latest.json"
            args.channel, args.build = "snapshot", "2026.1114.2"
            args.payload[0].installer = args.payload[0].installer.rename(args.payload[0].installer.parent / feed.installer_name(args.version, args.channel, args.build))
            args.payload[0].installer_url = f"https://github.com/{args.repository}/releases/download/snapshot/{args.payload[0].installer.name}"
            feed.generate(args)
            catalog = json.loads(args.input.read_text())
            self.assertEqual(catalog["stable"]["version"], "3.4.0")
            self.assertEqual(catalog["snapshot"]["build"], "2026.1114.2")
            enclosure = ET.parse(args.output / "snapshot/appcast.xml").find("channel/item/enclosure")
            self.assertEqual(enclosure.get(f"{{{feed.SPARKLE}}}version"), "3.4.0-snapshot.2026.1114.2")
            args.build = "2026.1114.1"
            args.payload[0].installer = args.payload[0].installer.rename(args.payload[0].installer.parent / feed.installer_name(args.version, args.channel, args.build))
            args.payload[0].installer_url = f"https://github.com/{args.repository}/releases/download/snapshot/{args.payload[0].installer.name}"
            with self.assertRaisesRegex(ValueError, "older"):
                feed.generate(args)

    def test_snapshot_filename_cannot_advertise_another_build(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.args(Path(directory), "snapshot", build="2026.1114.1")
            args.build = "2026.1114.2"
            with self.assertRaisesRegex(ValueError, "filename does not match"):
                feed.generate(args)

    def test_newer_stable_updates_both_native_feeds_and_retains_snapshot_history(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            snapshot = self.args(root, "snapshot", build="2026.1114.2")
            feed.generate(snapshot)
            stable = self.args(root, version="3.5.0")
            stable.input = stable.output / "latest.json"
            feed.generate(stable)
            catalog = json.loads(stable.input.read_text())
            self.assertEqual(catalog["snapshot"]["baseVersion"], "3.4.0")
            self.assertEqual(catalog["snapshot"]["build"], "2026.1114.2")
            for channel in ("stable", "snapshot"):
                enclosure = ET.parse(stable.output / channel / "appcast.xml").find("channel/item/enclosure")
                self.assertEqual(enclosure.get(f"{{{feed.SPARKLE}}}version"), "3.5.0")
                self.assertEqual(enclosure.get("url"), stable.payload[0].installer_url)

    def test_stable_without_snapshot_initializes_both_native_feeds(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.args(Path(directory))
            feed.generate(args)
            self.assertEqual((args.output / "stable/appcast.xml").read_bytes(),
                (args.output / "snapshot/appcast.xml").read_bytes())
            self.assertIsNone(json.loads((args.output / "latest.json").read_text())["snapshot"])

    def test_stable_does_not_replace_same_or_newer_base_snapshot_feed(self):
        for snapshot_version in ("3.4.0", "3.5.0"):
            with self.subTest(snapshot_version=snapshot_version), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                snapshot = self.args(root, "snapshot", version=snapshot_version, build="2026.1114.2")
                feed.generate(snapshot)
                before = (snapshot.output / "snapshot/appcast.xml").read_bytes()
                stable = self.args(root)
                stable.input = stable.output / "latest.json"
                feed.generate(stable)
                self.assertEqual((stable.output / "snapshot/appcast.xml").read_bytes(), before)

    def test_snapshot_older_than_stable_is_rejected_without_changing_published_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            stable = self.args(root, version="3.5.0")
            feed.generate(stable)
            published = {relative: (stable.output / relative).read_bytes()
                for relative in ("latest.json", "stable/appcast.xml", "snapshot/appcast.xml")}
            snapshot = self.args(root, "snapshot", version="3.4.0", build="2026.1114.3")
            snapshot.input = stable.output / "latest.json"
            with self.assertRaisesRegex(ValueError, "snapshot base version is older"):
                feed.generate(snapshot)
            for relative, content in published.items():
                self.assertEqual((stable.output / relative).read_bytes(), content)

    def test_private_loopback_download_requires_explicit_opt_in(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.args(Path(directory))
            args.payload[0].installer_url = "http://127.0.0.1:18770/setup.exe"
            with self.assertRaisesRegex(ValueError, "matching GitHub"):
                feed.generate(args)
            args.allow_localhost = True
            feed.generate(args)

    def test_missing_or_malformed_signature_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.args(Path(directory))
            args.payload[0].signature_file.write_text("not a signature")
            with self.assertRaises(ValueError):
                feed.generate(args)

    def test_preservation_keeps_other_feeds_and_does_not_hide_server_errors(self):
        class Response:
            def __enter__(self): return self
            def __exit__(self, *unused): pass
            def read(self, length): return b'{"schemaVersion": 1}'
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with patch.object(preserver, "urlopen", return_value=Response()) as request:
                preserver.preserve("https://updates.gpgui.com", root,
                    ["appcast.xml", "latest.json", "windows/stable/appcast.xml", "windows/snapshot/appcast.xml"])
                self.assertTrue((root / "windows/latest.json").exists())
                self.assertEqual(request.call_count, 1)
                outgoing = request.call_args.args[0]
                self.assertEqual(outgoing.full_url, "https://updates.gpgui.com/windows/latest.json")
                self.assertEqual(outgoing.get_header("User-agent"), "GPConnectReleasePublisher/1.0")
            with patch.object(preserver, "urlopen", side_effect=HTTPError("url", 500, "failure", {}, None)):
                with self.assertRaises(HTTPError):
                    preserver.preserve("https://updates.gpgui.com", root, [])


if __name__ == "__main__":
    unittest.main()

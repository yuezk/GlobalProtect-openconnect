import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).parents[1] / "validate-update-manifest.py"
SPEC = importlib.util.spec_from_file_location("validate_update_manifest", MODULE_PATH)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)

REPOSITORY = "yuezk/GlobalProtect-openconnect"


class UpdateManifestTests(unittest.TestCase):
    def stable(self, version="2.6.5"):
        return {
            "version": version,
            "releasedAt": "2026-08-01",
            "releaseUrl": f"https://github.com/{REPOSITORY}/releases/tag/v{version}",
            "platforms": ["linux", "freebsd", "openbsd"],
            "highlights": ["Stable release."],
        }

    def snapshot(self, base_version="2.6.5", build="17.1"):
        return {
            "baseVersion": base_version,
            "build": build,
            "releasedAt": "2026-08-07",
            "releaseUrl": f"https://github.com/{REPOSITORY}/releases/tag/snapshot",
            "platforms": ["linux", "freebsd", "openbsd"],
            "highlights": ["Development snapshot."],
        }

    def manifest(self):
        return {"schemaVersion": 1, "stable": self.stable(), "snapshot": self.snapshot()}

    def generate(self, directory, channel, version, build=None, input_manifest=None):
        directory = Path(directory)
        highlights = directory / f"{channel}-highlights.txt"
        output = directory / "latest.json"
        highlights.write_text(f"{channel.title()} change.\n", encoding="utf-8")
        args = type(
            "Args",
            (),
            {
                "repository": REPOSITORY,
                "channel": channel,
                "version": version,
                "build": build,
                "platform": None,
                "released_at": "2026-08-07",
                "highlights_file": highlights,
                "input": input_manifest,
                "output": output,
            },
        )()
        MODULE.generate(args)
        return json.loads(output.read_text(encoding="utf-8"))

    def test_validates_catalog_with_both_channels(self):
        result = MODULE.validate_manifest(self.manifest(), REPOSITORY)

        self.assertEqual(result["stable"]["version"], "2.6.5")
        self.assertEqual(result["snapshot"]["build"], "17.1")

    def test_accepts_catalog_with_one_channel(self):
        manifest = self.manifest()
        manifest["stable"] = None

        MODULE.validate_manifest(manifest, REPOSITORY)

    def test_rejects_empty_catalog_and_unknown_fields(self):
        with self.assertRaisesRegex(ValueError, "stable or snapshot"):
            MODULE.validate_manifest({"schemaVersion": 1, "stable": None, "snapshot": None}, REPOSITORY)

        manifest = self.manifest()
        manifest["extra"] = True
        with self.assertRaisesRegex(ValueError, "top-level"):
            MODULE.validate_manifest(manifest, REPOSITORY)

    def test_rejects_invalid_channel_urls(self):
        manifest = self.manifest()
        manifest["stable"]["releaseUrl"] = f"https://github.com/{REPOSITORY}/releases/tag/snapshot"
        with self.assertRaisesRegex(ValueError, "stable releaseUrl"):
            MODULE.validate_manifest(manifest, REPOSITORY)

        manifest = self.manifest()
        manifest["snapshot"]["releaseUrl"] = f"https://github.com/{REPOSITORY}/releases/tag/v2.6.5"
        with self.assertRaisesRegex(ValueError, "snapshot releaseUrl"):
            MODULE.validate_manifest(manifest, REPOSITORY)

    def test_rejects_invalid_snapshot_build(self):
        for build in ("", "17.beta", "01.1", "1.2.3.4", f"{2**64}.1"):
            with self.subTest(build=build):
                manifest = self.manifest()
                manifest["snapshot"]["build"] = build
                with self.assertRaisesRegex(ValueError, "snapshot build"):
                    MODULE.validate_manifest(manifest, REPOSITORY)

    def test_rejects_version_components_larger_than_the_client_supports(self):
        manifest = self.manifest()
        manifest["stable"] = self.stable(f"{2**64}.0.0")

        with self.assertRaisesRegex(ValueError, "integer range"):
            MODULE.validate_manifest(manifest, REPOSITORY)

    def test_rejects_unknown_and_duplicate_platforms(self):
        manifest = self.manifest()
        manifest["stable"]["platforms"] = ["linux", "netbsd"]
        with self.assertRaisesRegex(ValueError, "unsupported"):
            MODULE.validate_manifest(manifest, REPOSITORY)

        manifest["stable"]["platforms"] = ["linux", "linux"]
        with self.assertRaisesRegex(ValueError, "duplicates"):
            MODULE.validate_manifest(manifest, REPOSITORY)

    def test_generates_and_merges_channels(self):
        with tempfile.TemporaryDirectory() as directory:
            generated = self.generate(directory, "stable", "2.6.5")
            self.assertIsNone(generated["snapshot"])
            self.assertEqual(generated["stable"]["platforms"], ["linux", "freebsd", "openbsd"])

            current = Path(directory) / "current.json"
            current.write_text(json.dumps(generated), encoding="utf-8")
            generated = self.generate(directory, "snapshot", "2.6.5", "17.1", current)

            self.assertEqual(generated["stable"]["version"], "2.6.5")
            self.assertEqual(generated["snapshot"]["baseVersion"], "2.6.5")
            self.assertEqual(generated["snapshot"]["build"], "17.1")

    def test_cli_limits_stable_to_published_platforms_and_preserves_snapshot(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            current = directory / "current.json"
            current.write_text(json.dumps(self.manifest()), encoding="utf-8")
            highlights = directory / "highlights.txt"
            highlights.write_text("Refreshed client icons.\n", encoding="utf-8")
            output = directory / "latest.json"
            result = subprocess.run(
                [sys.executable, str(MODULE_PATH), "generate", "--repository", REPOSITORY,
                 "--channel", "stable", "--version", "3.0.5", "--platform", "linux",
                 "--released-at", "2026-10-13", "--highlights-file", str(highlights),
                 "--input", str(current), "--output", str(output)],
                capture_output=True, text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            generated = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(generated["stable"]["platforms"], ["linux"])
            self.assertEqual(generated["snapshot"], self.snapshot())

    def test_rejects_stable_and_snapshot_rollbacks(self):
        with tempfile.TemporaryDirectory() as directory:
            current = Path(directory) / "current.json"
            current.write_text(json.dumps(self.manifest()), encoding="utf-8")

            with self.assertRaisesRegex(ValueError, "stable version"):
                self.generate(directory, "stable", "2.6.4", input_manifest=current)
            with self.assertRaisesRegex(ValueError, "snapshot release"):
                self.generate(directory, "snapshot", "2.6.5", "16.1", current)

    def test_allows_new_stable_to_supersede_snapshot_base(self):
        with tempfile.TemporaryDirectory() as directory:
            current = Path(directory) / "current.json"
            current.write_text(json.dumps(self.manifest()), encoding="utf-8")

            generated = self.generate(directory, "stable", "2.6.6", input_manifest=current)

            self.assertEqual(generated["stable"]["version"], "2.6.6")
            self.assertEqual(generated["snapshot"]["baseVersion"], "2.6.5")


if __name__ == "__main__":
    unittest.main()

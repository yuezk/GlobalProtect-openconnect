import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("macos_release", Path(__file__).parents[1] / "validate-macos-release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class MacOSReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.commit = "a" * 40
        self.metadata = {"gp_ref": "b" * 40, "gpgui_ref": "d" * 40, "workflow_sha": self.commit, "channel": "stable",
                         "release_tag": "v3.0.1", "artifact_version": "3.0.1",
                         "request_id": "42.1.macos", "build_version": "903.1", "assets": {}}
        for extension in ("dmg", "zip"):
            name = f"GPConnect_3.0.1_arm64.{extension}"
            content = b"signed artifact"
            digest = hashlib.sha256(content).hexdigest()
            (self.directory / name).write_bytes(content)
            checksum = f"{digest}  {name}\n".encode()
            (self.directory / (name + ".sha256")).write_bytes(checksum)
            self.metadata["assets"][name] = digest
            self.metadata["assets"][name + ".sha256"] = hashlib.sha256(checksum).hexdigest()
        self.save()

    def save(self):
        (self.directory / "release-build.json").write_text(json.dumps(self.metadata))

    def test_matching_build_passes(self):
        self.assertEqual(release.validate(self.directory, "v3.0.1", self.commit), self.metadata)

    def test_another_release_cannot_use_this_build(self):
        with self.assertRaisesRegex(ValueError, "release tag"):
            release.validate(self.directory, "v3.0.2", self.commit)

    def test_another_run_commit_cannot_use_this_build(self):
        with self.assertRaisesRegex(ValueError, "public workflow"):
            release.validate(self.directory, "v3.0.1", "c" * 40)

    def test_source_commit_cannot_replace_workflow_identity(self):
        with self.assertRaisesRegex(ValueError, "public workflow"):
            release.validate(self.directory, "v3.0.1", self.metadata["gp_ref"])

    def test_invalid_workflow_commit_is_rejected(self):
        self.metadata["workflow_sha"] = "short"
        self.save()
        with self.assertRaisesRegex(ValueError, "Invalid workflow_sha"):
            release.validate(self.directory, "v3.0.1", "short")

    def test_corrupt_download_is_rejected(self):
        (self.directory / "GPConnect_3.0.1_arm64.zip").write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            release.validate(self.directory, "v3.0.1", self.commit)

    def test_unexpected_file_is_rejected(self):
        (self.directory / "unexpected.dmg").write_bytes(b"extra")
        with self.assertRaisesRegex(ValueError, "Unexpected"):
            release.validate(self.directory, "v3.0.1", self.commit)

    def test_missing_checksum_is_rejected(self):
        self.metadata["assets"].pop("GPConnect_3.0.1_arm64.zip.sha256")
        self.save()
        with self.assertRaisesRegex(ValueError, "exactly"):
            release.validate(self.directory, "v3.0.1", self.commit)

    def test_snapshot_requires_matching_build_identity(self):
        self.metadata.update(channel="snapshot", release_tag="snapshot",
                             artifact_version="3.0.1-snapshot.903.2")
        self.save()
        with self.assertRaisesRegex(ValueError, "snapshot artifact version"):
            release.validate(self.directory, "snapshot", self.commit)

    def test_matching_snapshot_passes(self):
        version = "3.0.1-snapshot.903.1"
        assets = {}
        for name in self.metadata["assets"]:
            file = self.directory / name
            new_name = name.replace("3.0.1", version)
            content = file.read_bytes()
            if name.endswith(".sha256"):
                content = content.replace(b"3.0.1", version.encode())
            file.unlink()
            (self.directory / new_name).write_bytes(content)
            assets[new_name] = hashlib.sha256(content).hexdigest()
        self.metadata.update(channel="snapshot", release_tag="snapshot",
                             artifact_version=version, assets=assets)
        self.save()
        self.assertEqual(release.validate(self.directory, "snapshot", self.commit), self.metadata)


if __name__ == "__main__":
    unittest.main()

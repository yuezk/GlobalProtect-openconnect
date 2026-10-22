import subprocess
import tempfile
import unittest
from pathlib import Path


REPOSITORY = "yuezk/GlobalProtect-openconnect"
SCRIPT = Path(__file__).parents[1] / "validate-macos-appcast.py"


def item(version, short_version, archive, channel=None):
    channel_xml = f"<sparkle:channel>{channel}</sparkle:channel>" if channel else ""
    return f"""
    <item>
      <sparkle:version>{version}</sparkle:version>
      <sparkle:shortVersionString>{short_version}</sparkle:shortVersionString>
      {channel_xml}
      <enclosure url="{archive}" length="100" sparkle:edSignature="signature" />
    </item>
    """


def appcast(*items):
    return f"""<?xml version="1.0" encoding="utf-8"?>
    <rss xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
      <channel>{''.join(items)}</channel>
    </rss>
    """


class ValidateMacosAppcastTests(unittest.TestCase):
    def run_validator(self, content, *arguments):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "appcast.xml"
            path.write_text(content, encoding="utf-8")
            return subprocess.run(
                ["python3", str(SCRIPT), *arguments, str(path), "--repository", REPOSITORY],
                check=False,
                capture_output=True,
                text=True,
            )

    def test_reports_candidate_status(self):
        archive = f"https://github.com/{REPOSITORY}/releases/download/v2.6.4/GPConnect_2.6.4_arm64.zip"
        content = appcast(item("806.1", "2.6.4", archive))

        same = self.run_validator(
            content, "status", "--build-version", "806.1"
        )
        newer = self.run_validator(
            content, "status", "--build-version", "807.1"
        )
        older = self.run_validator(
            content, "status", "--build-version", "805.1"
        )

        self.assertEqual((same.returncode, same.stdout.strip()), (0, "same"))
        self.assertEqual((newer.returncode, newer.stdout.strip()), (0, "newer"))
        self.assertEqual((older.returncode, older.stdout.strip()), (0, "older"))

    def test_validates_stable_and_snapshot_items(self):
        stable = f"https://github.com/{REPOSITORY}/releases/download/v2.6.4/GPConnect_2.6.4_arm64.zip"
        snapshot = (
            f"https://github.com/{REPOSITORY}/releases/download/snapshot/"
            "GPConnect_2.6.4-snapshot.807.1_arm64.zip"
        )
        content = appcast(
            item("806.1", "2.6.4", stable),
            item("807.1", "2.6.4", snapshot, "snapshot"),
        )

        result = self.run_validator(
            content,
            "validate",
            "--build-version",
            "807.1",
            "--short-version",
            "2.6.4",
            "--channel",
            "snapshot",
            "--archive-url",
            snapshot,
        )

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_compares_builds_across_channels(self):
        stable = f"https://github.com/{REPOSITORY}/releases/download/v2.6.4/GPConnect_2.6.4_arm64.zip"
        snapshot = (
            f"https://github.com/{REPOSITORY}/releases/download/snapshot/"
            "GPConnect_2.6.4-snapshot.807.1_arm64.zip"
        )
        content = appcast(
            item("2.6.4", "2.6.4", stable),
            item("807.1", "2.6.4", snapshot, "snapshot"),
        )

        result = self.run_validator(
            content, "status", "--build-version", "2.7.0"
        )

        self.assertEqual((result.returncode, result.stdout.strip()), (0, "older"))

    def test_rejects_external_archive_urls(self):
        content = appcast(item("806.1", "2.6.4", "https://example.com/update.zip"))

        result = self.run_validator(
            content, "status", "--build-version", "807.1"
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unexpected update archive URL", result.stderr)


if __name__ == "__main__":
    unittest.main()

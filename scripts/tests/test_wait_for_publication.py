import argparse
import gzip
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import HTTPError
import xml.etree.ElementTree as ET

spec = importlib.util.spec_from_file_location("publication", Path(__file__).parents[1] / "wait-for-publication.py")
publication = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publication)


def arguments(**values):
    return argparse.Namespace(owner="yuezk", project="globalprotect-openconnect",
                              package="globalprotect-openconnect", version="3.0.1", revision="1", **values)


class RepositoryTests(unittest.TestCase):
    def test_debian_index_requires_exact_version_and_architecture(self):
        content = b"Package: globalprotect-openconnect\nVersion: 3.0.1-ppa1\nArchitecture: arm64\n\n"
        self.assertTrue(publication.deb_present(content, "globalprotect-openconnect", "3.0.1-ppa1", "arm64"))
        self.assertFalse(publication.deb_present(content, "globalprotect-openconnect", "3.0.1", "arm64"))
        self.assertFalse(publication.deb_present(content, "globalprotect-openconnect", "3.0.1-ppa1", "amd64"))

    def test_rpm_metadata_is_verified_before_parsing(self):
        primary = gzip.compress(b'<metadata xmlns="http://linux.duke.edu/metadata/common"/>')
        digest = hashlib.sha256(primary).hexdigest()
        repomd = ET.fromstring(f'''<repomd xmlns="http://linux.duke.edu/metadata/repo">
          <data type="primary"><location href="repodata/primary.xml.gz"/>
          <checksum type="sha256">{digest}</checksum></data></repomd>''')
        with patch.object(publication, "xml", return_value=repomd), patch.object(publication, "fetch", return_value=primary):
            self.assertEqual(publication.rpm_packages("https://repo").tag, "{http://linux.duke.edu/metadata/common}metadata")
        with patch.object(publication, "xml", return_value=repomd), patch.object(publication, "fetch", return_value=b"corrupt"):
            with self.assertRaises(publication.Pending):
                publication.rpm_packages("https://repo")

    def test_copr_same_version_from_another_build_does_not_pass(self):
        primary = ET.fromstring('''<metadata xmlns="http://linux.duke.edu/metadata/common"><package>
          <name>globalprotect-openconnect</name><arch>x86_64</arch><version ver="3.0.1" rel="1.fc44"/>
          <location href="011106217-globalprotect-openconnect/package.rpm"/></package></metadata>''')
        with patch.object(publication, "rpm_packages", return_value=primary):
            params = ("https://repo", "globalprotect-openconnect", "3.0.1", "x86_64", lambda release: True)
            self.assertTrue(publication.rpm_present(*params, build_id=11106217))
            self.assertFalse(publication.rpm_present(*params, build_id=11106218))


class LaunchpadTests(unittest.TestCase):
    def setUp(self):
        self.checker = publication.PPA(arguments(series="noble", architectures=["amd64", "arm64"]))
        self.checker.source = "https://api/sourcepub/1"
        self.builds = [{"arch_tag": arch, "buildstate": "Successfully built", "web_link": f"https://build/{arch}"}
                       for arch in ("amd64", "arm64")]

    def test_green_builds_do_not_mean_repository_is_published(self):
        with patch.object(publication, "api", return_value={"status": "Published"}), \
                patch.object(publication, "collection", return_value=self.builds), \
                patch.object(publication, "fetch", return_value=gzip.compress(b"")):
            with self.assertRaisesRegex(publication.Pending, "Waiting for 3.0.1"):
                self.checker.check()

    def test_missing_architecture_does_not_pass(self):
        self.builds = self.builds[:1]
        with patch.object(publication, "api", return_value={"status": "Published"}), \
                patch.object(publication, "collection", return_value=self.builds), \
                patch.object(publication, "deb_present", return_value=True), \
                patch.object(publication, "fetch", return_value=gzip.compress(b"")):
            with self.assertRaisesRegex(publication.Pending, "arm64"):
                self.checker.check()

    def test_failed_build_reports_external_url(self):
        self.builds[0]["buildstate"] = "Dependency wait"
        with patch.object(publication, "api", return_value={"status": "Published"}), \
                patch.object(publication, "collection", return_value=self.builds):
            with self.assertRaisesRegex(publication.Failed, "https://build/amd64"):
                self.checker.check()


class OBSTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        files = Path(self.directory.name) / "files"
        files.mkdir()
        (files / "package.spec").write_bytes(b"spec")
        args = arguments(files=files)
        args.project = "home:yuezk"
        self.checker = publication.OBS(args)
        digest = hashlib.md5(b"spec").hexdigest()
        self.source = f'<directory rev="74" srcmd5="new"><entry name="package.spec" md5="{digest}"/></directory>'
        self.results = '''<resultlist><result repository="16.0" arch="x86_64" state="published">
          <status code="succeeded"/></result><result repository="disabled" arch="x86_64">
          <status code="disabled"/></result></resultlist>'''

    def test_previous_green_build_history_is_not_current_publication(self):
        responses = [self.source, self.results, '<buildinfo><srcmd5>new</srcmd5></buildinfo>',
                     '<buildhistory><entry srcmd5="old" versrel="3.0.1-1"/></buildhistory>']
        with patch.object(publication, "xml", side_effect=[ET.fromstring(value) for value in responses]):
            with self.assertRaisesRegex(publication.Pending, "build history"):
                self.checker.check()

    def test_current_revision_with_published_metadata_passes(self):
        responses = [self.source, self.results, '<buildinfo><srcmd5>new</srcmd5></buildinfo>',
                     '<buildhistory><entry srcmd5="new" versrel="3.0.1-1"/></buildhistory>']
        with patch.object(publication, "xml", side_effect=[ET.fromstring(value) for value in responses]), \
                patch.object(publication, "rpm_present", return_value=True):
            self.assertIn("1 targets", self.checker.check())

    def test_source_changed_during_wait_fails(self):
        self.checker.source = "old"
        with patch.object(publication, "xml", return_value=ET.fromstring(self.source)):
            with self.assertRaisesRegex(publication.Failed, "source changed"):
                self.checker.check()

    def test_no_enabled_targets_cannot_pass(self):
        with patch.object(publication, "xml", side_effect=[ET.fromstring(self.source), ET.fromstring("<resultlist/>")]):
            with self.assertRaisesRegex(publication.Failed, "no enabled"):
                self.checker.check()

    def test_enabled_target_outside_selection_fails(self):
        path = Path(self.directory.name) / "targets.json"
        path.write_text(json.dumps(["16.0/aarch64"]))
        self.checker.args.targets_file = path
        self.checker.targets = {("16.0", "aarch64")}
        with patch.object(publication, "xml", side_effect=[ET.fromstring(self.source), ET.fromstring(self.results)]):
            with self.assertRaisesRegex(publication.Failed, "unselected targets"):
                self.checker.check()

    def test_selected_target_disabled_fails(self):
        self.checker.targets = {("disabled", "x86_64")}
        with patch.object(publication, "xml", side_effect=[ET.fromstring(self.source), ET.fromstring(self.results)]):
            with self.assertRaisesRegex(publication.Failed, "selected target.*disabled"):
                self.checker.check()


class CoprTests(unittest.TestCase):
    def test_unselected_chroot_cannot_be_hidden_by_verification(self):
        checker = publication.Copr(arguments(build_id=42, chroots=["fedora-44-x86_64"]))
        build = {"ownername": "yuezk", "projectname": "globalprotect-openconnect",
                 "chroots": ["fedora-44-x86_64", "fedora-44-aarch64"]}
        with patch.object(publication, "api", return_value=build):
            with self.assertRaisesRegex(publication.Failed, "targets differ"):
                checker.check()

    def test_build_from_another_project_fails(self):
        checker = publication.Copr(arguments(build_id=42))
        with patch.object(publication, "api", return_value={"ownername": "other", "projectname": "other"}):
            with self.assertRaisesRegex(publication.Failed, "another project"):
                checker.check()

    def test_failed_build_reports_id_and_url(self):
        checker = publication.Copr(arguments(build_id=42))
        with patch.object(publication, "api", return_value={"ownername": "yuezk", "projectname": "globalprotect-openconnect", "state": "failed"}):
            with self.assertRaisesRegex(publication.Failed, "/build/42/"):
                checker.check()


class WaitTests(unittest.TestCase):
    def test_transient_error_then_publication(self):
        with patch.object(publication.time, "sleep"), patch.object(publication.time, "monotonic", return_value=0):
            callback = unittest.mock.Mock(side_effect=[HTTPError("https://api", 503, "", {}, None), "published"])
            self.assertEqual(publication.wait(callback, 60, 5), "published")

    def test_timeout_reports_last_external_status(self):
        with patch.object(publication.time, "monotonic", side_effect=[0, 61]):
            callback = unittest.mock.Mock(side_effect=publication.Pending("build 42 is running"))
            with self.assertRaisesRegex(publication.Failed, "build 42 is running"):
                publication.wait(callback, 60, 5)


if __name__ == "__main__":
    unittest.main()

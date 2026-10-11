import argparse
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import MagicMock, patch
import xml.etree.ElementTree as ET

spec = importlib.util.spec_from_file_location("selector", Path(__file__).parents[1] / "select-publication-targets.py")
selector = importlib.util.module_from_spec(spec)
spec.loader.exec_module(selector)


class SelectionTests(unittest.TestCase):
    def test_unknown_and_duplicate_targets_fail(self):
        for values in (["typo"], ["fedora-44", "fedora-44"]):
            with self.assertRaises(ValueError):
                selector.selection(values, {"fedora-44", "fedora-45"})

    def test_obs_metadata_uses_authenticated_put_without_authenticating_public_reads(self):
        response = MagicMock()
        response.__enter__.return_value.read.return_value = b"ok"
        with patch.object(selector, "urlopen", return_value=response) as opening, \
                patch.dict(selector.os.environ, {"OBS_USERNAME": "test", "OBS_PASSWORD": "test"}):
            selector.request("https://api.opensuse.org/source/project/package/_meta", b"<package/>")
            upload = opening.call_args.args[0]
            self.assertEqual(upload.get_method(), "PUT")
            self.assertIsNotNone(upload.get_header("Authorization"))
            selector.request("https://api.opensuse.org/public/source/project/package/_meta")
            public = opening.call_args.args[0]
            self.assertEqual(public.get_method(), "GET")
            self.assertIsNone(public.get_header("Authorization"))

    def test_copr_expands_selected_distro_to_all_its_configured_architectures(self):
        args = argparse.Namespace(owner="yuezk", project="globalprotect-openconnect",
                                  distros=["fedora-44"], build_id=None)
        response = json.dumps({"chroot_repos": {"fedora-44-aarch64": "url", "fedora-44-x86_64": "url",
                                               "fedora-45-x86_64": "url"}}).encode()
        with patch.object(selector, "request", return_value=response):
            self.assertEqual(selector.copr(args), ["fedora-44-aarch64", "fedora-44-x86_64"])
            args.distros = ["fedora-typo"]
            with self.assertRaisesRegex(ValueError, "Unknown targets"):
                selector.copr(args)

    def test_copr_recovery_uses_original_build_targets_even_if_project_changed(self):
        args = argparse.Namespace(owner="yuezk", project="globalprotect-openconnect", distros=[], build_id=42)
        response = json.dumps({"ownername": args.owner, "projectname": args.project,
                               "chroots": ["fedora-43-x86_64"]}).encode()
        with patch.object(selector, "request", return_value=response) as request:
            self.assertEqual(selector.copr(args), ["fedora-43-x86_64"])
            self.assertIn("/build/42", request.call_args.args[0])


class OBSSelectionTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.args = argparse.Namespace(project="home:yuezk", package="globalprotect-openconnect",
                                       repositories=["16.0"], verify_only=False,
                                       output=Path(self.directory.name) / "targets.json")
        self.targets = ["16.0/aarch64", "16.0/x86_64"]
        self.project = b'''<project><repository name="16.0"><arch>x86_64</arch><arch>aarch64</arch></repository>
          <repository name="15.6"><arch>x86_64</arch><arch>aarch64</arch></repository></project>'''
        self.package = b'''<package><title>Keep me</title><build><disable repository="old"/></build>
          <publish><disable repository="old"/><disable repository="16.0" arch="x86_64"/></publish>
          <useforbuild><disable arch="armv7l"/></useforbuild></package>'''

    def test_metadata_changes_build_allowlist_but_keeps_other_publication_flags(self):
        metadata = selector.obs_metadata(ET.fromstring(self.package), self.targets)
        self.assertEqual(metadata.findtext("title"), "Keep me")
        self.assertIsNotNone(metadata.find("build/disable"))
        self.assertEqual([flag.attrib for flag in metadata.findall("build/enable")],
                         [{"repository": "16.0", "arch": "aarch64"},
                          {"repository": "16.0", "arch": "x86_64"}])
        self.assertEqual(metadata.find("publish/disable").attrib, {"repository": "old"})
        self.assertEqual(len(metadata.findall("publish/disable")), 1)
        self.assertEqual(metadata.find("useforbuild/disable").get("arch"), "armv7l")

    def test_selection_is_applied_before_source_can_be_uploaded(self):
        before = {"16.0/x86_64": "succeeded", "16.0/aarch64": "succeeded",
                  "15.6/x86_64": "succeeded", "15.6/aarch64": "succeeded"}
        after = {"16.0/x86_64": "succeeded", "16.0/aarch64": "succeeded",
                 "15.6/x86_64": "disabled", "15.6/aarch64": "disabled"}
        with patch.object(selector, "request", side_effect=[self.project, self.package, b"ok"]) as request, \
                patch.object(selector, "obs_results", side_effect=[before, after]):
            selector.obs(self.args)
            self.assertEqual(json.loads(self.args.output.read_text()), self.targets)
            self.assertIn("/source/home:yuezk/globalprotect-openconnect/_meta", request.call_args.args[0])
            uploaded = ET.fromstring(request.call_args.args[1])
            self.assertEqual({flag.get("arch") for flag in uploaded.findall("build/enable")}, {"x86_64", "aarch64"})

    def test_busy_unselected_target_prevents_mutation(self):
        states = {"16.0/x86_64": "succeeded", "16.0/aarch64": "succeeded", "15.6/aarch64": "building"}
        with patch.object(selector, "request", return_value=self.project) as request, \
                patch.object(selector, "obs_results", return_value=states):
            with self.assertRaisesRegex(ValueError, "still active"):
                selector.obs(self.args)
            self.assertEqual(request.call_count, 1)

    def test_verify_only_does_not_mutate_to_hide_an_unselected_build(self):
        self.args.verify_only = True
        states = {"16.0/x86_64": "succeeded", "16.0/aarch64": "succeeded", "15.6/aarch64": "succeeded"}
        with patch.object(selector, "request", return_value=self.project) as request, \
                patch.object(selector, "obs_results", return_value=states):
            with self.assertRaisesRegex(ValueError, "differ"):
                selector.obs(self.args)
            self.assertEqual(request.call_count, 1)

    def test_empty_selection_preserves_current_flags(self):
        self.args.repositories = []
        states = {"16.0/x86_64": "succeeded", "16.0/aarch64": "disabled"}
        with patch.object(selector, "request", return_value=self.project) as request, \
                patch.object(selector, "obs_results", return_value=states):
            self.assertEqual(selector.obs(self.args), ["16.0/x86_64"])
            self.assertEqual(request.call_count, 1)


if __name__ == "__main__":
    unittest.main()

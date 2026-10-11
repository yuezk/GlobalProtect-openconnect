import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).parents[1] / "verify-docker-publication.sh"
AMD64 = "sha256:" + "a" * 64
ARM64 = "sha256:" + "b" * 64
INDEX = "sha256:" + "c" * 64


def manifest(digest, architecture):
    platform_os = "unknown" if architecture == "unknown" else "linux"
    return {"digest": digest, "platform": {"os": platform_os, "architecture": architecture}}


class DockerPublicationTests(unittest.TestCase):
    def run_verifier(self, images, source_index=True, selection="all"):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            digests = root / "digests"
            digests.mkdir()
            (digests / (INDEX if source_index else AMD64)[7:]).touch()
            if selection == "all":
                (digests / ARM64[7:]).touch()
            responses = {
                "example/image:3.0.1": {"manifests": images},
                "example/image@" + INDEX: {"manifests": [manifest(AMD64, "amd64"), manifest("sha256:attestation", "unknown")]},
                "example/image@" + AMD64: {"config": {}},
                "example/image@" + ARM64: {"config": {}},
            }
            (root / "responses.json").write_text(json.dumps(responses))
            docker = root / "docker"
            docker.write_text("#!/usr/bin/env python3\nimport json, os, sys\n"
                              "print(json.dumps(json.load(open(os.environ['RESPONSES']))[sys.argv[4]]))\n")
            docker.chmod(0o755)
            sleep = root / "sleep"
            sleep.write_text("#!/bin/sh\nexit 0\n")
            sleep.chmod(0o755)
            env = {**os.environ, "PATH": str(root) + os.pathsep + os.environ["PATH"],
                   "RESPONSES": str(root / "responses.json")}
            return subprocess.run(["bash", str(SCRIPT), "example/image:3.0.1", str(digests), selection],
                                  env=env, capture_output=True, text=True)

    def test_attested_sources_match_the_published_platform_digests(self):
        result = self.run_verifier([manifest(AMD64, "amd64"), manifest(ARM64, "arm64"),
                                    manifest("sha256:attestation", "unknown")])
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_missing_architecture_cannot_pass(self):
        result = self.run_verifier([manifest(AMD64, "amd64")])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("did not match", result.stderr)

    def test_another_build_of_same_platforms_cannot_pass(self):
        result = self.run_verifier([manifest("sha256:" + "d" * 64, "amd64"), manifest(ARM64, "arm64")])
        self.assertNotEqual(result.returncode, 0)

    def test_unselected_platform_cannot_pass(self):
        result = self.run_verifier([manifest(AMD64, "amd64"), manifest(ARM64, "arm64"),
                                    manifest("sha256:extra", "riscv64")])
        self.assertNotEqual(result.returncode, 0)

    def test_selected_single_architecture_is_supported(self):
        result = self.run_verifier([manifest(AMD64, "amd64")], source_index=False, selection="x86_64")
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()

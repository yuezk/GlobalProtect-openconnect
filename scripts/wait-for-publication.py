#!/usr/bin/env python3
"""Wait for an exact release to build and appear in public package repositories."""

import argparse
import bz2
import gzip
import hashlib
import json
import lzma
import os
from pathlib import Path
import re
import subprocess
import sys
import time
from urllib.error import HTTPError, URLError
from urllib.parse import quote, urlencode, urljoin
from urllib.request import Request, urlopen
import xml.etree.ElementTree as ET


class Pending(Exception):
    """The external build or repository is not ready yet."""


class Failed(Exception):
    """The requested publication failed or was superseded."""


def fetch(url):
    request = Request(url, headers={"Cache-Control": "no-cache", "User-Agent": "gp-release-verifier"})
    with urlopen(request, timeout=30) as response:
        return response.read()


def api(url, **parameters):
    if parameters:
        url += "?" + urlencode(parameters)
    return json.loads(fetch(url))


def collection(url, **parameters):
    result = api(url, **parameters)
    entries = result["entries"]
    while result.get("next_collection_link"):
        result = api(result["next_collection_link"])
        entries.extend(result["entries"])
    return entries


def xml(url):
    return ET.fromstring(fetch(url))


def unpack(data, name):
    if name.endswith(".gz"):
        return gzip.decompress(data)
    if name.endswith(".xz"):
        return lzma.decompress(data)
    if name.endswith(".bz2"):
        return bz2.decompress(data)
    if name.endswith(".zst"):
        return subprocess.run(["zstd", "--decompress", "--stdout"], input=data,
                              check=True, capture_output=True).stdout
    return data


def check_digest(data, algorithm, expected):
    if not expected or hashlib.new(algorithm, data).hexdigest() != expected:
        raise Pending("Repository metadata checksum does not match yet")


def rpm_packages(base_url):
    """Read primary metadata through repomd, verifying its advertised checksum."""
    namespace = {"repo": "http://linux.duke.edu/metadata/repo"}
    metadata = xml(base_url + "/repodata/repomd.xml")
    primary = metadata.find("repo:data[@type='primary']", namespace)
    if primary is None:
        raise Pending(f"No primary repository metadata at {base_url}")
    location = primary.find("repo:location", namespace).get("href")
    checksum = primary.find("repo:checksum", namespace)
    content = fetch(urljoin(base_url + "/", location))
    check_digest(content, checksum.get("type"), checksum.text)
    return ET.fromstring(unpack(content, location))


def rpm_present(base_url, package, version, arch, release_matches, build_id=None):
    namespace = {"rpm": "http://linux.duke.edu/metadata/common"}
    for entry in rpm_packages(base_url).findall("rpm:package", namespace):
        info = entry.find("rpm:version", namespace)
        location = entry.find("rpm:location", namespace).get("href")
        if (entry.findtext("rpm:name", namespaces=namespace) != package or
                entry.findtext("rpm:arch", namespaces=namespace) != arch or
                info.get("ver") != version or not release_matches(info.get("rel", ""))):
            continue
        if build_id is not None and not re.search(rf"(?:^|/)0*{build_id}-", location):
            continue
        return True
    return False


def deb_present(content, package, version, arch):
    for paragraph in content.decode().split("\n\n"):
        fields = dict(line.split(": ", 1) for line in paragraph.splitlines()
                      if ": " in line and not line.startswith(" "))
        if (fields.get("Package") == package and fields.get("Version") == version and
                fields.get("Architecture") in {arch, "all"}):
            return True
    return False


class PPA:
    def __init__(self, args):
        self.args = args
        self.archive = f"https://api.launchpad.net/1.0/~{args.owner}/+archive/ubuntu/{args.project}"
        self.series = f"https://api.launchpad.net/1.0/ubuntu/{args.series}"
        self.source = None

    def check(self):
        args = self.args
        if self.source is None:
            sources = collection(self.archive, **{
                "ws.op": "getPublishedSources", "source_name": args.package,
                "exact_match": "true", "version": args.version, "distro_series": self.series,
            })
            sources = [source for source in sources if source["source_package_version"] == args.version]
            if not sources:
                raise Pending(f"Waiting for source acceptance: {args.series} {args.version}")
            self.source = sources[0]["self_link"]
            print(f"Launchpad source: {self.source}", flush=True)
        source = api(self.source)
        if source["status"] in {"Deleted", "Superseded", "Obsolete"}:
            raise Failed(f"Launchpad source is {source['status']}: {self.source}")
        builds = collection(self.source, **{"ws.op": "getBuilds"})
        states = {build["arch_tag"]: build for build in builds}
        for arch in args.architectures:
            build = states.get(arch)
            if build is None:
                raise Pending(f"Waiting for Launchpad {arch} build: {self.source}")
            state = build["buildstate"]
            link = build["web_link"]
            if state in {"Failed to build", "Dependency wait", "Chroot problem", "Build for superseded Source",
                         "Failed to upload", "Cancelled"}:
                raise Failed(f"Launchpad {arch}: {state}; {link}")
            if state != "Successfully built":
                raise Pending(f"Launchpad {arch}: {state}; {link}")
        for arch in args.architectures:
            link = states[arch]["web_link"]
            base = f"https://ppa.launchpadcontent.net/{args.owner}/{args.project}/ubuntu"
            url = f"{base}/dists/{args.series}/main/binary-{arch}/Packages.gz"
            if not deb_present(gzip.decompress(fetch(url)), args.package, args.version, arch):
                raise Pending(f"Waiting for {args.version} in {url}; build {link}")
        return f"PPA {args.series}: {args.version} published for {', '.join(args.architectures)}; {self.source}"


class OBS:
    def __init__(self, args):
        self.args = args
        self.source_url = (f"https://api.opensuse.org/public/source/{quote(args.project, safe=':')}/"
                           f"{args.package}")
        self.source = None
        self.targets = None
        if getattr(args, "targets_file", None):
            targets = json.loads(args.targets_file.read_text())
            if not targets or any(not isinstance(target, str) or target.count("/") != 1 for target in targets):
                raise Failed("Invalid OBS target selection file")
            self.targets = {tuple(target.split("/")) for target in targets}

    def check(self):
        args = self.args
        source = xml(self.source_url)
        if self.source is None:
            # Bind the waiter to the uploaded files, not whichever revision is currently green.
            remote = {entry.get("name"): entry.get("md5") for entry in source.findall("entry")}
            for file in args.files.iterdir():
                if file.is_file() and remote.get(file.name) != hashlib.md5(file.read_bytes()).hexdigest():
                    raise Failed(f"OBS uploaded source does not match {file.name}: {self.source_url}")
            self.source = source.get("srcmd5")
            if not self.source:
                raise Failed("OBS source has no srcmd5")
            print(f"OBS revision {source.get('rev')} ({self.source}): {self.source_url}", flush=True)
        elif source.get("srcmd5") != self.source:
            raise Failed(f"OBS source changed during verification: {self.source_url}")

        results = xml(f"https://api.opensuse.org/public/build/{quote(args.project, safe=':')}/_result"
                      f"?{urlencode({'package': args.package, 'view': 'status'})}")
        targets = {(result.get("repository"), result.get("arch")): result for result in results}
        active = {target for target, result in targets.items()
                  if result.find("status") is not None and
                  result.find("status").get("code") not in {"disabled", "excluded"}}
        if getattr(args, "targets_file", None) and active - self.targets:
            raise Failed("OBS enabled unselected targets during publication: " +
                         ", ".join(f"{repo}/{arch}" for repo, arch in sorted(active - self.targets)))
        if self.targets is None:
            self.targets = active
            if not self.targets:
                raise Failed("OBS has no enabled build targets")
        for repo, arch in sorted(self.targets):
            result = targets.get((repo, arch))
            if result is None or result.find("status") is None:
                raise Failed(f"OBS target disappeared: {repo}/{arch}")
            state = result.find("status").get("code")
            build_url = f"https://api.opensuse.org/public/build/{quote(args.project, safe=':')}/{repo}/{arch}/{args.package}"
            if state in {"disabled", "excluded"}:
                raise Failed(f"OBS selected target {repo}/{arch}: {state}; {build_url}")
            info = xml(build_url + "/_buildinfo")
            if info.findtext("srcmd5") != self.source:
                raise Pending(f"Waiting for OBS revision {self.source}: {repo}/{arch}; {build_url}")
            if state in {"failed", "unresolvable", "broken", "disabled", "excluded"}:
                raise Failed(f"OBS {repo}/{arch}: {state}; {build_url}/_log")
            if state != "succeeded" or result.get("state") != "published":
                raise Pending(f"OBS {repo}/{arch}: build {state}, repository {result.get('state')}; {build_url}")
            history = xml(build_url + "/_history")
            entries = history.findall("entry")
            if not entries or entries[-1].get("srcmd5") != self.source:
                raise Pending(f"Waiting for successful OBS build history: {repo}/{arch}")
            if entries[-1].get("versrel") != f"{args.version}-{args.revision}":
                raise Failed(f"OBS built an unexpected version: {repo}/{arch}")
            project_path = args.project.replace(":", ":/")
            public = f"https://download.opensuse.org/repositories/{project_path}/{repo}"
            pattern = rf"(?:[A-Za-z0-9_]+\.)?{re.escape(args.revision)}\.\d+"
            if not rpm_present(public, args.package, args.version, arch,
                               lambda release: re.fullmatch(pattern, release) is not None):
                raise Pending(f"Waiting for OBS {args.version}-{args.revision} in {public} ({arch})")
        return f"OBS revision {self.source}: {len(self.targets)} targets built and published; {self.source_url}"


class Copr:
    def __init__(self, args):
        self.args = args
        self.build_url = f"https://copr.fedorainfracloud.org/api_3/build/{args.build_id}"

    def check(self):
        args = self.args
        build = api(self.build_url)
        link = f"https://copr.fedorainfracloud.org/coprs/{args.owner}/{args.project}/build/{args.build_id}/"
        if build["ownername"] != args.owner or build["projectname"] != args.project:
            raise Failed(f"Copr build belongs to another project: {link}")
        if getattr(args, "chroots", None) and set(build["chroots"]) != set(args.chroots):
            raise Failed(f"Copr build targets differ from the requested selection: {link}")
        if build["state"] in {"failed", "canceled", "cancelled", "skipped"}:
            raise Failed(f"Copr build {build['state']}: {link}")
        if build["state"] != "succeeded":
            raise Pending(f"Copr build {build['state']}: {link}")
        if build["source_package"]["version"] != f"{args.version}-{args.revision}":
            raise Failed(f"Copr build version does not match requested release: {link}")
        if build["source_package"]["name"] != args.package or not build["chroots"]:
            raise Failed(f"Unexpected Copr package or empty target list: {link}")
        for chroot in build["chroots"]:
            status = api(f"https://copr.fedorainfracloud.org/api_3/build-chroot/{args.build_id}/{chroot}")
            if status["state"] in {"failed", "canceled", "cancelled", "skipped"}:
                raise Failed(f"Copr {chroot}: {status['state']}; {link}")
            if status["state"] != "succeeded":
                raise Pending(f"Copr {chroot}: {status['state']}; {link}")
            base = build["repo_url"].rstrip("/") + "/" + chroot
            arch = chroot.rsplit("-", 1)[1]
            if not rpm_present(base, args.package, args.version, arch,
                               lambda release: release == args.revision or release.startswith(args.revision + "."),
                               build_id=args.build_id):
                raise Pending(f"Waiting for Copr build {args.build_id} in {base}")
        return f"Copr build {args.build_id}: {len(build['chroots'])} targets built and published; {link}"


def wait(check, timeout, interval):
    deadline = time.monotonic() + timeout
    last_message = None
    while True:
        try:
            return check()
        except HTTPError as error:
            if error.code != 404 and error.code != 429 and error.code < 500:
                raise Failed(f"Publication API returned HTTP {error.code}: {error.url}") from error
            message = f"Waiting after HTTP {error.code}: {error.url}"
        except (Pending, URLError, TimeoutError) as error:
            message = str(error)
        if message != last_message:
            print(message, flush=True)
            last_message = message
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise Failed(f"Publication timed out after {timeout}s: {message}")
        time.sleep(min(interval, remaining))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--timeout", type=int, default=10800, help="Maximum wait in seconds")
    parser.add_argument("--interval", type=int, default=60, help="Poll interval in seconds")
    subparsers = parser.add_subparsers(dest="service", required=True)
    for name, implementation in (("ppa", PPA), ("obs", OBS), ("copr", Copr)):
        subparser = subparsers.add_parser(name)
        subparser.set_defaults(implementation=implementation)
        subparser.add_argument("--owner", default="yuezk")
        subparser.add_argument("--project", default="home:yuezk" if name == "obs" else "globalprotect-openconnect")
        subparser.add_argument("--package", default="globalprotect-openconnect")
        subparser.add_argument("--version", required=True)
        if name == "ppa":
            subparser.add_argument("--series", required=True)
            subparser.add_argument("--architectures", nargs="+", default=["amd64", "arm64"])
        else:
            subparser.add_argument("--revision", default="1")
        if name == "obs":
            subparser.add_argument("--files", type=Path, required=True)
            subparser.add_argument("--targets-file", type=Path)
        if name == "copr":
            subparser.add_argument("--build-id", type=int, required=True)
            subparser.add_argument("--chroots", nargs="+")
    args = parser.parse_args()
    if args.timeout <= 0 or args.interval <= 0:
        parser.error("timeout and interval must be positive")
    try:
        result = wait(args.implementation(args).check, args.timeout, args.interval)
    except (Failed, ValueError, OSError, ET.ParseError, subprocess.CalledProcessError) as error:
        print(str(error), file=sys.stderr)
        return 1
    print(result)
    if summary := os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(summary, "a", encoding="utf-8") as file:
            file.write(result + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())

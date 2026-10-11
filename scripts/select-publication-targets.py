#!/usr/bin/env python3
"""Restrict distribution submissions to the targets selected for this rollout."""

import argparse
import base64
import json
import os
from pathlib import Path
import re
import sys
import time
from urllib.parse import quote, urlencode
from urllib.request import Request, urlopen
import xml.etree.ElementTree as ET


def request(url, data=None):
    headers = {"User-Agent": "gp-release-target-selector", "Cache-Control": "no-cache"}
    if data is not None:
        credentials = f"{os.environ['OBS_USERNAME']}:{os.environ['OBS_PASSWORD']}"
        headers["Authorization"] = "Basic " + base64.b64encode(credentials.encode()).decode()
        headers["Content-Type"] = "application/xml"
    method = "PUT" if data is not None else "GET"
    with urlopen(Request(url, data=data, headers=headers, method=method), timeout=30) as response:
        return response.read()


def selection(values, available):
    if not values:
        values = sorted(available)
    if not values or len(values) != len(set(values)):
        raise ValueError("Select at least one target, without duplicates")
    unknown = set(values) - set(available)
    if unknown:
        raise ValueError(f"Unknown targets: {', '.join(sorted(unknown))}; available: {', '.join(sorted(available))}")
    return sorted(values)


def copr(args):
    if args.build_id:
        if args.build_id <= 0:
            raise ValueError("Copr build ID must be positive")
        build = json.loads(request(f"https://copr.fedorainfracloud.org/api_3/build/{args.build_id}"))
        if build["ownername"] != args.owner or build["projectname"] != args.project:
            raise ValueError("Copr build belongs to another project")
        available = build["chroots"]
    else:
        query = urlencode({"ownername": args.owner, "projectname": args.project})
        project = json.loads(request(f"https://copr.fedorainfracloud.org/api_3/project?{query}"))
        available = project["chroot_repos"]
    if any(not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", name) for name in available):
        raise ValueError("Copr returned an invalid chroot name")
    distros = selection(args.distros, {chroot.rsplit("-", 1)[0] for chroot in available})
    return sorted(chroot for chroot in available if chroot.rsplit("-", 1)[0] in distros)


def obs_results(project, package):
    query = urlencode({"package": package, "view": "status"})
    root = ET.fromstring(request(f"https://api.opensuse.org/public/build/{project}/_result?{query}"))
    return {f"{result.get('repository')}/{result.get('arch')}": result.find("status").get("code")
            for result in root if result.find("status") is not None}


def obs_metadata(metadata, targets):
    # Keep publication flags for unselected targets so their existing binaries stay available.
    build = metadata.find("build")
    if build is None:
        build = ET.SubElement(metadata, "build")
    build.clear()
    ET.SubElement(build, "disable")
    publish = metadata.find("publish")
    if publish is None:
        publish = ET.SubElement(metadata, "publish")
    for target in targets:
        repo, arch = target.split("/")
        attributes = {"repository": repo, "arch": arch}
        ET.SubElement(build, "enable", attributes)
        for flag in list(publish):
            if flag.attrib == attributes:
                publish.remove(flag)
        ET.SubElement(publish, "enable", attributes)
    return metadata


def obs(args):
    project = quote(args.project, safe=":")
    package = quote(args.package, safe="")
    project_metadata = ET.fromstring(request(f"https://api.opensuse.org/public/source/{project}/_meta"))
    available = {f"{repo.get('name')}/{arch.text}" for repo in project_metadata.findall("repository")
                 for arch in repo.findall("arch")}
    states = obs_results(project, package)
    active = {target for target, state in states.items() if state not in {"disabled", "excluded"}}
    if args.repositories:
        repositories = selection(args.repositories, {target.split("/")[0] for target in available})
        targets = sorted(target for target in available if target.split("/")[0] in repositories)
    else:
        targets = selection(sorted(active), available)
    if args.verify_only:
        if set(targets) != active:
            raise ValueError("OBS enabled targets differ from the requested verification selection; no flags changed")
    elif args.repositories:
        # Do not let an already-running unselected build publish during a rollout change.
        busy = {target for target in active - set(targets)
                if states[target] not in {"succeeded", "failed", "broken", "unresolvable"}}
        if busy:
            raise ValueError(f"Unselected OBS targets are still active: {', '.join(sorted(busy))}; wait before changing selection")
        url = f"https://api.opensuse.org/source/{project}/{package}/_meta"
        public_url = f"https://api.opensuse.org/public/source/{project}/{package}/_meta"
        metadata = ET.fromstring(request(public_url))
        updated = obs_metadata(metadata, targets)
        request(url, ET.tostring(updated, encoding="utf-8"))
        # Confirm the scheduler applied the allowlist before committing new source.
        deadline = time.monotonic() + 120
        while True:
            states = obs_results(project, package)
            active = {target for target, state in states.items() if state not in {"disabled", "excluded"}}
            if active == set(targets):
                break
            if time.monotonic() >= deadline:
                raise ValueError("OBS target flags have not taken effect; source was not uploaded; flags remain selected")
            time.sleep(5)
    args.output.write_text(json.dumps(targets) + "\n")
    return targets


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="service", required=True)
    copr_parser = commands.add_parser("copr")
    copr_parser.add_argument("--distros", type=str.split, default=[])
    copr_parser.add_argument("--owner", default="yuezk")
    copr_parser.add_argument("--project", default="globalprotect-openconnect")
    copr_parser.add_argument("--build-id", type=int)
    obs_parser = commands.add_parser("obs")
    obs_parser.add_argument("--repositories", type=str.split, default=[])
    obs_parser.add_argument("--project", default="home:yuezk")
    obs_parser.add_argument("--package", default="globalprotect-openconnect")
    obs_parser.add_argument("--verify-only", action="store_true")
    obs_parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        targets = {"copr": copr, "obs": obs}[args.service](args)
    except (ValueError, KeyError, OSError, ET.ParseError) as error:
        print(str(error), file=sys.stderr)
        return 1
    message = f"Selected {args.service} targets: {' '.join(targets)}"
    print(message)
    if output := os.environ.get("GITHUB_OUTPUT"):
        with open(output, "a") as file:
            file.write(f"targets={' '.join(targets)}\n")
    if summary := os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(summary, "a") as file:
            file.write(message + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())

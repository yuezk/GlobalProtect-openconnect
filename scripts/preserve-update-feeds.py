#!/usr/bin/env python3
"""Retain other platform/channel feeds before replacing a static Worker deployment."""
import argparse
import json
from pathlib import Path
from urllib.error import HTTPError
from urllib.parse import urlparse
from urllib.request import urlopen
import xml.etree.ElementTree as ET

FEEDS = ("appcast.xml", "latest.json", "windows/latest.json", "windows/stable/appcast.xml", "windows/snapshot/appcast.xml")


def preserve(base_url: str, directory: Path, exclude: list[str]) -> None:
    if urlparse(base_url).scheme != "https":
        raise ValueError("feed host must use HTTPS")
    for relative in FEEDS:
        if relative in exclude:
            continue
        try:
            with urlopen(base_url.rstrip("/") + "/" + relative, timeout=30) as response:
                data = response.read(1024 * 1024 + 1)
        except HTTPError as error:
            if error.code == 404:
                continue
            raise
        if len(data) > 1024 * 1024:
            raise ValueError(f"published feed is too large: {relative}")
        if relative.endswith(".json"):
            json.loads(data)
        else:
            ET.fromstring(data)
        target = directory / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-url", required=True)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--exclude", action="append", default=[], choices=FEEDS)
    args = parser.parse_args()
    preserve(args.base_url, args.directory, args.exclude)

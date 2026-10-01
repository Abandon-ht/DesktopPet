#!/usr/bin/env python3
"""Download an apt-get --print-uris plan on a connected preparation machine.

Usage: download-apt.py APT_PLAN OUTPUT_DIRECTORY
Transfer the resulting .deb files to the build machine; apt never needs to fetch
them there. The plan must come from that machine's current package database.
"""
import concurrent.futures
import hashlib
import json
from pathlib import Path
import shlex
import sys
import urllib.parse
import urllib.request


def download(row, directory):
    url, name, size, checksum = row
    parsed = urllib.parse.urlsplit(url)
    if parsed.scheme not in ("http", "https") or not parsed.hostname:
        raise ValueError(f"unsupported package URL: {url}")
    url = urllib.parse.urlunsplit(parsed._replace(scheme="https"))
    if Path(name).name != name or not name.endswith(".deb"):
        raise ValueError("invalid package filename")
    algorithm, expected = checksum.split(":", 1)
    if algorithm != "MD5Sum":
        raise ValueError(f"unsupported apt checksum: {algorithm}")
    destination = directory / name
    if not destination.exists():
        temporary = destination.with_suffix(".partial")
        with urllib.request.urlopen(url, timeout=90) as response, temporary.open("wb") as output:
            while chunk := response.read(1024 * 1024):
                output.write(chunk)
        temporary.replace(destination)
    data = destination.read_bytes()
    if len(data) != int(size) or hashlib.md5(data).hexdigest() != expected:
        destination.unlink()
        raise ValueError(f"apt package integrity mismatch: {name}")
    return dict(url=url, filename=name, bytes=len(data), sha256=hashlib.sha256(data).hexdigest())


def main():
    plan, output = map(Path, sys.argv[1:])
    output.mkdir(parents=True, exist_ok=True)
    rows = [shlex.split(line) for line in plan.read_text().splitlines() if line.startswith("'")]
    if not rows or any(len(row) != 4 for row in rows):
        raise ValueError("no valid package URI plan")
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
        results = list(pool.map(lambda row: download(row, output), rows))
    (output / "SHA256SUMS").write_text("".join(f"{r['sha256']}  {r['filename']}\n" for r in results))
    (output / "manifest.json").write_text(json.dumps(results, indent=2) + "\n")
    print(f"Verified {len(results)} packages ({sum(r['bytes'] for r in results)} bytes)")


if __name__ == "__main__":
    main()

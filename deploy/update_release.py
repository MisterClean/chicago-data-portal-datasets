#!/usr/bin/env python3
"""Pull tested public GitHub releases; no deployment or Bluesky credentials needed."""

import fcntl
import hashlib
import json
import os
import re
import shutil
import subprocess
import tempfile
import urllib.request
from pathlib import Path

REPOSITORY = "MisterClean/chicago-data-portal-datasets"
BINARY = "chicago-data-bot-linux-amd64"
TAG_PATTERN = r"build-[0-9a-f]{40}"


def download(url: str, destination: Path, limit: int) -> str:
    request = urllib.request.Request(url, headers={"User-Agent": "chicago-data-bot-deploy/1"})
    digest = hashlib.sha256()
    total = 0
    with urllib.request.urlopen(request, timeout=45) as response, destination.open("wb") as output:
        while chunk := response.read(64 * 1024):
            total += len(chunk)
            if total > limit:
                raise ValueError("Release asset exceeded download size limit")
            digest.update(chunk)
            output.write(chunk)
        output.flush()
        os.fsync(output.fileno())
    return digest.hexdigest()


def validate_checksum(contents: str) -> str:
    fields = contents.split()
    if len(fields) != 2 or re.fullmatch(r"[0-9a-f]{64}", fields[0]) is None or fields[1] != BINARY:
        raise ValueError("Unexpected checksum manifest")
    return fields[0]


def switch_link(root: Path, name: str, target: Path) -> None:
    pending = root / f".{name}-next"
    pending.unlink(missing_ok=True)
    pending.symlink_to(target)
    pending.replace(root / name)


def update(root: Path, state: Path) -> None:
    # A root-owned pause file is useful during manual rollback/incident response.
    if Path("/etc/chicago-data-bot/deploy-paused").exists():
        print("Deployment paused by operator")
        return
    with (state / "update.lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        install_latest(root)


def install_latest(root: Path) -> None:
    releases = root / "releases"
    releases.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".download-", dir=releases) as directory:
        work = Path(directory)
        metadata = work / "release.json"
        download(f"https://api.github.com/repos/{REPOSITORY}/releases/latest", metadata, 256_000)
        release = json.loads(metadata.read_text(encoding="utf-8"))
        tag = release["tag_name"]
        if re.fullmatch(TAG_PATTERN, tag) is None or release["draft"] or release["prerelease"]:
            raise ValueError("Latest release is not an eligible production build")
        target = releases / tag
        current = root / "current"
        if current.is_symlink() and current.resolve() == target:
            print(f"Already running {tag}")
            return
        assets = {asset["name"]: asset["browser_download_url"] for asset in release["assets"]}
        for name in (BINARY, "SHA256SUMS"):
            expected_url = f"https://github.com/{REPOSITORY}/releases/download/{tag}/{name}"
            if assets.get(name) != expected_url:
                raise ValueError(f"Missing or unexpected release asset: {name}")
        checksum_file = work / "SHA256SUMS"
        download(assets["SHA256SUMS"], checksum_file, 4096)
        expected = validate_checksum(checksum_file.read_text(encoding="utf-8"))
        binary = work / "chicago-data-bot"
        actual = download(assets[BINARY], binary, 32_000_000)
        if actual != expected:
            raise ValueError("Release checksum mismatch; existing binary remains active")
        binary.chmod(0o755)
        subprocess.run([str(binary), "--version"], check=True, timeout=15, cwd=work)
        if target.exists():
            # Reuse an immutable release left by an interrupted prior activation.
            with (target / "chicago-data-bot").open("rb") as existing:
                if hashlib.file_digest(existing, "sha256").hexdigest() != expected:
                    raise ValueError("Existing release directory has unexpected content")
        else:
            target.mkdir()
            binary.replace(target / "chicago-data-bot")
            metadata.replace(target / "release.json")
        if current.is_symlink():
            switch_link(root, "previous", current.resolve())
        switch_link(root, "current", target)
        # Persist the rename before reporting deployment success.
        descriptor = os.open(root, os.O_RDONLY)
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
        keep = {current.resolve(), (root / "previous").resolve()}
        for old in releases.iterdir():
            if old.is_dir() and not old.is_symlink() and re.fullmatch(TAG_PATTERN, old.name) is not None and old not in keep:
                shutil.rmtree(old)
        print(f"Installed {tag}; SHA256 {expected}")


if __name__ == "__main__":
    update(Path("/opt/chicago-data-bot"), Path("/var/lib/chicago-data-deploy"))

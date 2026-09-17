"""Deployment rejection paths must leave the active release untouched."""
import hashlib
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import update_release as updater


class ReleaseTests(unittest.TestCase):
    def test_malformed_checksum_is_rejected(self) -> None:
        for contents in ("", "bad  file", "a" * 64 + "  ../other", "a" * 64 + "  " + updater.BINARY + " extra"):
            with self.assertRaises(ValueError):
                updater.validate_checksum(contents)

    def test_checksum_mismatch_preserves_current(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            previous = root / "old"
            previous.mkdir()
            (root / "current").symlink_to(previous)
            tag = "build-" + "a" * 40
            release = {"tag_name": tag, "draft": False, "prerelease": False, "assets": [
                {"name": name, "browser_download_url": f"https://github.com/{updater.REPOSITORY}/releases/download/{tag}/{name}"}
                for name in (updater.BINARY, "SHA256SUMS")
            ]}

            def download(url: str, destination: Path, limit: int) -> str:
                if url.endswith("/latest"):
                    contents = json.dumps(release).encode()
                elif url.endswith("SHA256SUMS"):
                    contents = ("0" * 64 + "  " + updater.BINARY).encode()
                else:
                    contents = b"bad binary"
                destination.write_bytes(contents)
                return hashlib.sha256(contents).hexdigest()

            with patch.object(updater, "download", side_effect=download):
                with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                    updater.install_latest(root)
            self.assertEqual((root / "current").resolve(), previous.resolve())
            self.assertEqual(list((root / "releases").iterdir()), [])


if __name__ == "__main__":
    unittest.main()

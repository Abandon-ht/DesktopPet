"""Public-package privacy and architecture guards; no private test assets."""
import contextlib
import io
import json
from pathlib import Path
import struct
import tempfile
import unittest

import appimage


def elf(path, machine=62):
    header = bytearray(20)
    header[:6] = b"\x7fELF\x02\x01"
    struct.pack_into("<H", header, 18, machine)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(header)


class PublicPackageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.binaries = self.root / "binaries"
        for name in ("desktop-pet", "avatar-host-2d"):
            elf(self.binaries / name)
        self.loader = self.root / "libvulkan.so.1"
        elf(self.loader)
        self.stage = self.root / "stage"

    def prepare(self):
        with contextlib.redirect_stdout(io.StringIO()):
            appimage.prepare(self.binaries, self.stage, self.loader)

    def appdir(self):
        self.prepare()
        directory = self.root / "AppDir"
        resources = directory / "usr/bin/resources"
        resources.mkdir(parents=True)
        for path in (self.stage / "resources").iterdir():
            (resources / path.name).write_bytes(path.read_bytes())
        for name in ("desktop-pet", "avatar-host-2d"):
            elf(directory / "usr/bin" / name)
        for name in ("usr/lib/libvulkan.so.1", "usr/lib/WebKitWebProcess", "usr/lib/WebKitNetworkProcess"):
            elf(directory / name)
        (directory / "AppRun").write_text("#!/bin/sh\n")
        (directory / "AppRun").chmod(0o755)
        commit = json.loads((resources / "release.json").read_text())["source_commit"]
        return directory, commit

    def test_only_allowlisted_resources_and_sidecar_are_staged(self):
        (self.binaries / "private.onnx").write_text("never publish")
        (self.binaries / ".env").write_text("never publish")
        self.prepare()
        config = json.loads((self.stage / "tauri-appimage.conf.json").read_text())
        self.assertEqual(set(p.name for p in (self.stage / "resources").iterdir()), appimage.RESOURCE_NAMES)
        self.assertNotIn("dev-data-dir.txt", str(config))
        self.assertFalse(list(self.stage.rglob("*.onnx")))
        self.assertFalse(list(self.stage.rglob(".env")))

    def test_refuses_to_overwrite_staging(self):
        self.stage.mkdir()
        marker = self.stage / "keep"
        marker.write_text("existing")
        with self.assertRaises(ValueError):
            self.prepare()
        self.assertEqual(marker.read_text(), "existing")

    def test_rejects_macos_binary_before_staging(self):
        (self.binaries / "desktop-pet").write_bytes(b"Mach-O")
        with self.assertRaises(ValueError):
            self.prepare()
        self.assertFalse(self.stage.exists())

    def test_rejects_other_architecture_before_staging(self):
        elf(self.binaries / "avatar-host-2d", machine=183)
        with self.assertRaises(ValueError):
            self.prepare()
        self.assertFalse(self.stage.exists())

    def test_clean_public_tree_passes(self):
        directory, commit = self.appdir()
        self.assertEqual(appimage.audit(directory, commit)["source_commit"], commit)

    def test_rejects_unexpected_resource_and_user_data(self):
        directory, commit = self.appdir()
        (directory / "usr/bin/resources/dev-data-dir.txt").write_text("/private")
        with self.assertRaises(ValueError):
            appimage.audit(directory, commit)

    def test_rejects_private_assets_outside_resource_directory(self):
        directory, commit = self.appdir()
        (directory / "private.onnx").write_text("not redistributable")
        with self.assertRaises(ValueError):
            appimage.audit(directory, commit)

    def test_rejects_host_gpu_driver(self):
        directory, commit = self.appdir()
        elf(directory / "usr/lib/libnvidia-glcore.so.1")
        with self.assertRaises(ValueError):
            appimage.audit(directory, commit)

    def test_rejects_escaping_and_absolute_symlinks(self):
        directory, commit = self.appdir()
        link = directory / "leak"
        link.symlink_to("../../private")
        with self.assertRaises(ValueError):
            appimage.audit(directory, commit)
        link.unlink()
        link.symlink_to("/usr/lib")
        with self.assertRaises(ValueError):
            appimage.audit(directory, commit)

    def test_rejects_wrong_source_commit(self):
        directory, _ = self.appdir()
        with self.assertRaises(ValueError):
            appimage.audit(directory, "wrong-source")


if __name__ == "__main__":
    unittest.main()

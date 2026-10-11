"""AX650 AppImage architecture, native integrity and SDK exclusion regressions."""
import contextlib
import io
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import appimage
from test_tools import elf


class Ax650AppImageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.binaries = self.root / "binaries"
        self.binaries.mkdir()
        for name in ("desktop-pet", "avatar-host-2d"):
            (self.binaries / name).write_bytes(elf())
        self.loader = self.root / "libvulkan.so.1"
        self.loader.write_bytes(elf())
        self.native = self.root / "native"
        (self.native / "lib").mkdir(parents=True)
        (self.native / "licenses").mkdir()
        for name in ("libsherpa-onnx-c-api.so", "libonnxruntime.so.1.28.2"):
            (self.native / "lib" / name).write_bytes(elf())
        (self.native / "lib/libonnxruntime.so").symlink_to("libonnxruntime.so.1.28.2")
        for name in appimage.native.LICENSES:
            (self.native / "licenses" / name).write_text("Fixture license notice\n")
        self.commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=appimage.linux.ROOT, text=True).strip()
        self.provenance = self.root / "provenance.json"
        self.receipt = dict(source_commit=self.commit, sherpa_revision=appimage.native.REVISION,
                            sdk_revision="sdk-fixture", provider="axera", native_origin="CI fixture")
        self.provenance.write_text(json.dumps(self.receipt))
        self.assets = self.root / "assets"
        self.assets.mkdir()
        checks = {}
        for name in appimage.linux.ARCHIVES:
            with zipfile.ZipFile(self.assets / name, "w") as archive:
                if name == "avatar.zip":
                    archive.writestr("avatar/manifest.json", '{"renderer":"live2d_mocari"}')
                elif name == "voice.zip":
                    archive.writestr("voice/demo.wav", "fixture")
                else:
                    archive.writestr(f"models/{name}.onnx", "fixture")
            path = self.assets / name
            checks[name] = dict(sha256=appimage.linux.digest(path), bytes=path.stat().st_size)
        (self.assets / "SHA256SUMS.json").write_text(json.dumps(checks))
        self.axera_assets = self.root / "axera-assets"
        self.axera_assets.mkdir()
        model_root = self.root / "models"
        names = ["sensevoice/sensevoice.axmodel", "sensevoice/tokens.txt",
                 "zipvoice/encoder.axmodel", "zipvoice/vocos_full.axmodel",
                 "zipvoice/tokens.txt", "zipvoice/lexicon.txt", "zipvoice/espeak-ng-data/phondata",
                 *(f"zipvoice/decoder_part{index}.axmodel" for index in range(4))]
        for name in names:
            path = model_root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"matched model or frontend fixture")
        manifest = dict(version=1, encoder=dict(file="encoder.axmodel"),
                        decoder_parts=[dict(file=f"decoder_part{i}.axmodel") for i in range(4)])
        (model_root / "zipvoice/decoder4_split_manifest.json").write_text(json.dumps(manifest))
        model_files = appimage.models.selected_files(model_root)
        self.model_lock = dict(schema_version=1, resource_tag="fixture-ax650", sources=dict(origin="fixture"),
                               files={name: dict(sha256=appimage.models.digest(path), bytes=path.stat().st_size)
                                      for name, path in model_files.items()})
        self.model_lock["archive"] = appimage.models.write_archive(model_files, self.axera_assets / "models-axera.zip")
        self.lock_path = self.root / "model-lock.json"
        self.lock_path.write_text(json.dumps(self.model_lock))
        lock_patch = patch.object(appimage.models, "LOCK", self.lock_path)
        lock_patch.start()
        self.addCleanup(lock_patch.stop)
        self.write_model_checks()
        self.stage = self.root / "stage"

    def write_model_checks(self):
        archive = self.model_lock["archive"]
        (self.axera_assets / "SHA256SUMS.json").write_text(json.dumps(
            {archive["name"]: {key: archive[key] for key in ("sha256", "bytes")}}))

    def prepare(self):
        with contextlib.redirect_stdout(io.StringIO()):
            appimage.prepare(self.binaries, self.stage, self.assets, "fixture-resources",
                             self.native, self.provenance, self.loader, self.axera_assets, "fixture-ax650")

    def finalized(self):
        self.prepare()
        tree = self.root / "AppDir"
        config = json.loads((self.stage / "tauri-appimage.conf.json").read_text())
        for relative, source in config["bundle"]["linux"]["appimage"]["files"].items():
            target = tree / relative.lstrip("/")
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, target)
        for name in ("desktop-pet", "avatar-host-2d"):
            shutil.copy2(self.binaries / name, tree / "usr/bin" / name)
        for name in ("WebKitWebProcess", "WebKitNetworkProcess"):
            (tree / "usr/lib" / name).write_bytes(elf())
        (tree / "AppRun").write_text("#!/bin/sh\nexit 0\n")
        (tree / "AppRun").chmod(0o755)
        sdk = tree / "usr/lib/libax_engine.so"
        sdk.write_bytes(elf())
        with self.assertRaises(ValueError):
            appimage.linux.audit(tree, self.commit, "aarch64")
        def patch_rpath(arguments, **_kwargs):
            with Path(arguments[-1]).open("ab") as stream:
                stream.write(b"fixture changed RPATH")
        with patch.object(appimage.subprocess, "run", side_effect=patch_rpath):
            self.assertEqual(appimage.finalize_tree(tree), ["usr/lib/libax_engine.so"])
        return tree

    def test_final_image_excludes_sdk_and_records_transformed_native_bytes(self):
        tree = self.finalized()
        metadata = appimage.linux.audit(tree, self.commit, "aarch64")
        result = appimage.audit(tree, metadata)
        self.assertFalse(result["sdk_bundled"])
        self.assertFalse(list(tree.rglob("libax_*")))
        runtime = metadata["native_runtime"]
        self.assertNotEqual(runtime["build_files"]["usr/lib/libsherpa-onnx-c-api.so"],
                            runtime["files"]["usr/lib/libsherpa-onnx-c-api.so"])
        self.assertEqual(runtime["files"]["usr/lib/libonnxruntime.so"],
                         runtime["files"]["usr/lib/libonnxruntime.so.1.28.2"])

    def test_image_contains_locked_axmodels_and_complete_frontend(self):
        tree = self.finalized()
        metadata = appimage.linux.audit(tree, self.commit, "aarch64")
        result = appimage.audit(tree, metadata)
        self.assertTrue(metadata["bundled_axera_models"])
        self.assertEqual(len(list(tree.rglob("*.axmodel"))), 7)
        self.assertEqual(result["axera_models"]["file_count"], len(self.model_lock["files"]))
        (tree / "usr/bin/resources/models/axera/zipvoice/decoder_part3.axmodel").unlink()
        with self.assertRaises((ValueError, appimage.packaging.PackageError)):
            appimage.audit(tree, metadata)

    def test_remote_checksum_cannot_replace_locked_model_archive(self):
        archive = self.axera_assets / "models-axera.zip"
        with archive.open("ab") as stream:
            stream.write(b"changed Release bytes")
        checks = {archive.name: dict(sha256=appimage.models.digest(archive), bytes=archive.stat().st_size)}
        (self.axera_assets / "SHA256SUMS.json").write_text(json.dumps(checks))
        with self.assertRaises(ValueError):
            self.prepare()

    def test_model_archive_rejects_missing_part_or_unapproved_reference_audio(self):
        archive = self.axera_assets / "models-axera.zip"
        with zipfile.ZipFile(archive) as source:
            entries = {entry.filename: source.read(entry) for entry in source.infolist()}
        del entries["models/axera/zipvoice/decoder_part3.axmodel"]
        entries["models/axera/zipvoice/private-reference.wav"] = b"private voice"
        with zipfile.ZipFile(archive, "w") as destination:
            for name, content in entries.items():
                destination.writestr(name, content)
        self.model_lock["archive"].update(sha256=appimage.models.digest(archive), bytes=archive.stat().st_size)
        self.lock_path.write_text(json.dumps(self.model_lock))
        self.write_model_checks()
        with self.assertRaises(ValueError):
            self.prepare()

    def test_tampered_native_library_or_launcher_is_rejected(self):
        tree = self.finalized()
        metadata = appimage.linux.audit(tree, self.commit, "aarch64")
        path = tree / "usr/lib/libsherpa-onnx-c-api.so"
        original = path.read_bytes()
        path.write_bytes(original + b"tampered")
        with self.assertRaises(ValueError):
            appimage.audit(tree, metadata)
        path.write_bytes(original)
        (tree / "AppRun").write_text("#!/bin/sh\n")
        with self.assertRaises(ValueError):
            appimage.audit(tree, metadata)

    def test_foreign_native_revision_or_commit_is_rejected_before_staging(self):
        for key, value in (("sherpa_revision", "wrong"), ("source_commit", "wrong")):
            receipt = dict(self.receipt, **{key: value})
            self.provenance.write_text(json.dumps(receipt))
            with self.assertRaises(ValueError):
                self.prepare()
            self.assertFalse(self.stage.exists())

    def test_unexpected_native_library_and_escaping_symlink_are_rejected(self):
        path = self.native / "lib/libax_engine.so"
        path.write_bytes(elf())
        with self.assertRaises(ValueError):
            self.prepare()
        path.unlink()
        path = self.native / "lib/libsherpa-onnx-c-api.so"
        path.unlink()
        path.symlink_to(self.loader)
        with self.assertRaises(ValueError):
            self.prepare()
        self.assertFalse(self.stage.exists())

    def test_wrong_architecture_and_missing_license_are_rejected(self):
        path = self.native / "lib/libsherpa-onnx-c-api.so"
        path.write_bytes(elf(machine=62))
        with self.assertRaises(ValueError):
            self.prepare()
        path.write_bytes(elf())
        (self.native / "licenses" / appimage.native.LICENSES[0]).unlink()
        with self.assertRaises(ValueError):
            self.prepare()
        self.assertFalse(self.stage.exists())


if __name__ == "__main__":
    unittest.main()

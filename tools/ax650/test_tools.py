#!/usr/bin/env python3
"""Packaging and preflight regression tests; no role, display or device startup."""
import importlib.util
import json
from pathlib import Path
import struct
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zlib

HERE = Path(__file__).resolve().parent


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, HERE / filename)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


packaging = module("ax650_package", "package.py")
runtime = module("ax650_dependencies", "runtime-deps.py")
launcher = module("ax650_launcher", "launch.py")


def elf(machine=183, glibc="2.35"):
    """Minimal ELF64 fixture with actual Verneed/Vernaux and linked string table."""
    names = b"\0.shstrtab\0.dynstr\0.gnu.version_r\0"
    strings = b"\0libc.so.6\0GLIBC_" + glibc.encode("ascii") + b"\0"
    versions = struct.pack("<HHIII", 1, 1, 1, 16, 0) + struct.pack("<IHHII", 0, 0, 3, 11, 0)
    header = bytearray(64)
    header[:7] = b"\x7fELF\x02\x01\x01"
    struct.pack_into("<HH", header, 16, 3, machine)
    payload = names + strings + versions
    table_offset = 64 + len(payload)
    struct.pack_into("<Q", header, 40, table_offset)
    struct.pack_into("<HHH", header, 58, 64, 4, 1)
    sections = bytes(64)
    sections += struct.pack("<IIQQQQIIQQ", 1, 3, 0, 0, 64, len(names), 0, 0, 1, 0)
    sections += struct.pack("<IIQQQQIIQQ", 11, 3, 0, 0, 64 + len(names), len(strings), 0, 0, 1, 0)
    sections += struct.pack("<IIQQQQIIQQ", 19, 0x6ffffffe, 0, 0, 64 + len(names) + len(strings), len(versions), 2, 1, 8, 0)
    return bytes(header) + payload + sections


def png():
    def chunk(name, content):
        return struct.pack(">I", len(content)) + name + content + struct.pack(">I", zlib.crc32(name + content))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(b"\0\xff\xff\xff\xff")) + chunk(b"IEND", b""))


class PackageTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name).resolve()
        self.avatar = self.root / "avatar"
        (self.avatar / "model").mkdir(parents=True)
        self.model = {"Version": 3, "FileReferences": {"Moc": "model.moc3", "Textures": ["texture.png"]}}
        self.manifest = {
            "schema_version": 1, "renderer": "live2d_mocari", "entry": "model/model.model3.json",
            "interaction": {}, "actions": {"head_pat": {"expression": "model/happy.exp3.json"}},
            "license": {"status": "unverified", "redistributable": False},
        }
        (self.avatar / "model/model.moc3").write_bytes(b"MOC3fixture")
        (self.avatar / "model/texture.png").write_bytes(png())
        (self.avatar / "model/happy.exp3.json").write_text('{"Parameters": []}')
        self.write_manifests()
        self.binaries = self.root / "binaries"
        self.binaries.mkdir()
        for name in packaging.BINARIES:
            (self.binaries / name).write_bytes(elf())

    def tearDown(self):
        self.temporary.cleanup()

    def write_manifests(self):
        (self.avatar / "manifest.json").write_text(json.dumps(self.manifest))
        (self.avatar / "model/model.model3.json").write_text(json.dumps(self.model))

    def files(self):
        return packaging.avatar_files(self.avatar / "manifest.json")[1]

    def test_checks_actual_aarch64_and_required_glibc(self):
        path = self.binaries / packaging.BINARIES[0]
        self.assertEqual(packaging.verify_elf(path)["glibc_required"], "2.35")
        for data in (elf(machine=62), elf(glibc="2.36"), elf(glibc="2.35.1"), b"\xcf\xfa\xed\xfe"):
            path.write_bytes(data)
            with self.assertRaises(packaging.PackageError):
                packaging.verify_elf(path)

    def test_rejects_truncated_elf_version_requirements(self):
        path = self.binaries / packaging.BINARIES[0]
        path.write_bytes(elf()[:-1])
        with self.assertRaises(packaging.PackageError):
            packaging.verify_elf(path)

    def test_only_referenced_assets_enter_package(self):
        (self.avatar / "model/unreferenced.exp3.json").write_text("{}")
        (self.avatar / "credentials.txt").write_text("private")
        with patch.object(packaging, "source_metadata", return_value={
                "source_revision": "fixture", "source_branch": "ax650", "source_worktree_dirty": True}):
            output, archive = packaging.package(self.binaries, self.avatar / "manifest.json", None, self.root / "bundle")
        self.assertTrue((output / "resources/avatar/model/happy.exp3.json").is_file())
        self.assertFalse((output / "resources/avatar/model/unreferenced.exp3.json").exists())
        self.assertFalse((output / "resources/avatar/credentials.txt").exists())
        self.assertTrue((output / "SHA256SUMS.txt").is_file())
        manifest = (output / "package-manifest.json").read_text()
        self.assertNotIn(str(self.root), manifest)
        self.assertIn("source_worktree_dirty", manifest)
        with tarfile.open(archive) as tar:
            self.assertIn("bundle/run.sh", tar.getnames())
            self.assertTrue(all(not member.issym() for member in tar.getmembers()))

    def test_path_escape_and_absolute_references_fail_before_output(self):
        for reference in ("../outside.json", "/etc/passwd", "model/../outside.json", "model\\outside.json"):
            self.manifest["entry"] = reference
            self.write_manifests()
            output = self.root / "unsafe"
            with self.assertRaises(packaging.PackageError):
                packaging.package(self.binaries, self.avatar / "manifest.json", None, output)
            self.assertFalse(output.exists())

    def test_missing_model_and_expression_assets_are_rejected(self):
        (self.avatar / "model/happy.exp3.json").unlink()
        with self.assertRaises(packaging.PackageError):
            self.files()

    def test_file_and_directory_symlinks_rejected(self):
        (self.avatar / "unused-link").symlink_to(self.binaries, target_is_directory=True)
        with self.assertRaises(packaging.PackageError):
            self.files()
        (self.avatar / "unused-link").unlink()
        texture = self.avatar / "model/texture.png"
        texture.unlink()
        texture.symlink_to(self.binaries / packaging.BINARIES[0])
        with self.assertRaises(packaging.PackageError):
            self.files()

    def test_audio_motion_rejected(self):
        self.model["FileReferences"]["Motions"] = {"Idle": [{"File": "motion.json", "Sound": "sound.wav"}]}
        self.write_manifests()
        with self.assertRaises(packaging.PackageError):
            self.files()

    def test_existing_outputs_and_unignored_checkout_output_rejected(self):
        with self.assertRaises(packaging.PackageError):
            packaging.output_paths(self.avatar)
        with patch.object(packaging, "ROOT", self.root), patch.object(packaging.subprocess, "run") as run:
            run.return_value = subprocess.CompletedProcess([], 1)
            with self.assertRaises(packaging.PackageError):
                packaging.output_paths(self.root / "source-output")
            run.return_value = subprocess.CompletedProcess([], 0)
            self.assertEqual(packaging.output_paths(self.root / "ignored-output")[0], self.root / "ignored-output")

    def test_broken_output_symlink_is_rejected(self):
        output = self.root / "linked-output"
        output.symlink_to(self.root / "new-target")
        with self.assertRaises(packaging.PackageError):
            packaging.output_paths(output)
        self.assertFalse((self.root / "new-target").exists())

    def test_runtime_lib_whitelist_and_required_copyright(self):
        libraries = self.root / "libs"
        (libraries / "licenses").mkdir(parents=True)
        for name, notice in packaging.RUNTIME_LIBRARIES.items():
            (libraries / name).write_bytes(elf())
            (libraries / "licenses" / notice).write_text("Copyright fixture")
        (libraries / "libGL.so.1").write_bytes(b"do not bundle graphics drivers")
        files = packaging.runtime_files(libraries)
        self.assertEqual(len(files), 4)
        self.assertFalse(any(target.name == "libGL.so.1" for _, target in files))
        (libraries / "licenses/libxcb-xkb1.copyright").unlink()
        with self.assertRaises(packaging.PackageError):
            packaging.runtime_files(libraries)


class DependenciesTests(unittest.TestCase):
    def loader(self, name):
        return type("Library", (), {symbol: object() for symbol in runtime.XKB_SYMBOLS})()

    def test_gl_does_not_require_vulkan(self):
        result = runtime.check("gl", self.loader)
        self.assertEqual(result["failures"], [])
        self.assertIn("libEGL.so.1", result["loaded"])
        self.assertNotIn("libvulkan.so.1", result["loaded"])

    def test_vulkan_does_not_require_gl(self):
        result = runtime.check("vulkan", self.loader)
        self.assertIn("libvulkan.so.1", result["loaded"])
        self.assertNotIn("libEGL.so.1", result["loaded"])

    def test_missing_dynamic_xkb_library_or_symbol_fails(self):
        def missing_library(name):
            if name == "libxcb-xkb.so.1":
                raise OSError("missing library")
            return self.loader(name)
        self.assertEqual(runtime.check("gl", missing_library)["failures"][0]["library"], "libxcb-xkb.so.1")
        def missing_symbol(name):
            return object() if name == "libxkbcommon-x11.so.0" else self.loader(name)
        self.assertEqual(runtime.check("gl", missing_symbol)["failures"][0]["library"], "libxkbcommon-x11.so.0")


class LauncherTests(unittest.TestCase):
    def test_environment_preserves_authority_and_backend_isolation(self):
        env = launcher.environment(Path("/private-package"), "gl", {
            "DISPLAY": ":2", "XAUTHORITY": "/existing/session.auth", "LD_LIBRARY_PATH": "/existing/lib",
        })
        self.assertEqual(env["DISPLAY"], ":2")
        self.assertEqual(env["XAUTHORITY"], "/existing/session.auth")
        self.assertEqual(env["LD_LIBRARY_PATH"], "/private-package/lib:/existing/lib")
        self.assertNotIn("VK_ICD_FILENAMES", env)

    def test_check_only_failure_records_result_without_starting_rust(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(launcher, "ROOT", Path(directory)), \
             patch.object(launcher.subprocess, "run") as run, patch.object(launcher.os, "execve") as execute:
            run.return_value = subprocess.CompletedProcess([], 1, '{"failures": [{"library": "missing"}]}', "")
            self.assertEqual(launcher.main(["--check-only", "--log-dir", directory]), 1)
            result = json.loads((Path(directory) / "result.json").read_text())
            self.assertFalse(result["host_started"])
            self.assertFalse(result["completed"])
            execute.assert_not_called()

    def test_check_only_success_does_not_require_model_or_start_rust(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(launcher, "ROOT", Path(directory)), \
             patch.object(launcher.subprocess, "run") as run, patch.object(launcher.os, "execve") as execute:
            run.return_value = subprocess.CompletedProcess([], 0, '{"failures": []}', "")
            self.assertEqual(launcher.main(["--check-only", "--log-dir", directory]), 0)
            self.assertFalse(json.loads((Path(directory) / "result.json").read_text())["host_started"])
            execute.assert_not_called()


if __name__ == "__main__":
    unittest.main()

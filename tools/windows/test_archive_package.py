"""Verify privacy guards and portable ZIP layout without private signing data."""
from pathlib import Path
import tempfile
import unittest
import zipfile

from archive_package import archive


class ArchiveTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.package = self.root / 'DesktopPet'
        for name in ('desktop-pet.exe', 'avatar-host-2d.exe', 'DesktopPet-signing.cer',
                     'resources/avatar/manifest.json', 'start-test.ps1'):
            file = self.package / name
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_bytes(b'fixture')
        self.output = self.root / 'DesktopPet.zip'

    def test_portable_layout_and_post_signing_checksum(self):
        archive(self.package, self.output, 'test-commit')
        with zipfile.ZipFile(self.output) as zipped:
            self.assertIn('DesktopPet/resources/avatar/manifest.json', zipped.namelist())
            self.assertIn('DesktopPet/BUILD-INFO.json', zipped.namelist())
        self.assertIn('DesktopPet.zip', (self.root / 'SHA256SUMS.txt').read_text())

    def test_qa_configuration_and_private_files_are_rejected_before_archiving(self):
        for name in ('resources/voice-settings.json', 'resources/dev-data-dir.txt',
                     'resources/private.pfx', 'resources/care.sqlite3', 'github-private-key'):
            with self.subTest(name=name):
                file = self.package / name
                file.parent.mkdir(parents=True, exist_ok=True)
                file.write_bytes(b'private fixture')
                with self.assertRaises(ValueError):
                    archive(self.package, self.output, 'test-commit')
                self.assertFalse(self.output.exists())
                file.unlink()


if __name__ == '__main__':
    unittest.main()

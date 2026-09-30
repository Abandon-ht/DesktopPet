"""Exercise a real isolated installer, including upgrade and safe uninstallation."""
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest

from archive_package import archive
from make_installer import script


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.package = self.root / 'package'
        for name in ('desktop-pet.exe', 'avatar-host-2d.exe', 'DesktopPet-signing.cer',
                     'resources/avatar/manifest.json', 'start-test.ps1'):
            file = self.package / name
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_bytes(b'installer fixture')
        archive(self.package, self.root / 'portable.zip', 'fixture-commit')

    def test_private_resources_cannot_enter_installer(self):
        (self.package / 'resources' / 'voice-settings.json').write_text('private')
        with self.assertRaises(ValueError):
            script(self.package, self.root / 'Setup.exe', '0.1.0')

    @unittest.skipUnless(os.name == 'nt', 'Windows installer integration')
    def test_install_upgrade_autostart_and_uninstall_preserve_user_files(self):
        import winreg
        compiler = Path(r'C:\Program Files (x86)\NSIS\makensis.exe')
        if not compiler.is_file():
            self.skipTest('NSIS is not installed')
        app_id = 'DesktopPet-Installer-QA-' + str(os.getpid())
        run_key = r'Software\Microsoft\Windows\CurrentVersion\Run'
        product_key = 'Software\\' + app_id
        uninstall_key = r'Software\Microsoft\Windows\CurrentVersion\Uninstall' + '\\' + app_id
        setup = self.root / 'Setup.exe'
        source = self.root / 'setup.nsi'
        source.write_text(script(self.package, setup, '0.1.0', app_id), encoding='utf-8-sig')
        subprocess.run([str(compiler), '/V2', str(source)], check=True)
        install = self.root / '安装 目录 with spaces'

        def read(key, value):
            try:
                with winreg.OpenKey(winreg.HKEY_CURRENT_USER, key, 0,
                                    winreg.KEY_READ | winreg.KEY_WOW64_64KEY) as handle:
                    return winreg.QueryValueEx(handle, value)[0]
            except FileNotFoundError:
                return None

        def run_setup(*args):
            # NSIS requires /D to be last and unquoted, even for paths with spaces.
            command = subprocess.list2cmdline([str(setup), '/S', *args]) + ' /D=' + str(install)
            subprocess.run(command,
                           check=True, timeout=30)

        def uninstall():
            executable = install / 'Uninstall.exe'
            if executable.is_file():
                subprocess.run([str(executable), '/S'], check=True, timeout=30)
                deadline = time.monotonic() + 15
                while executable.exists() and time.monotonic() < deadline:
                    time.sleep(0.1)
                self.assertFalse(executable.exists(), 'uninstaller did not finish')

        self.assertIsNone(read(run_key, app_id), 'QA ID must be unused')
        try:
            run_setup()
            self.assertIsNone(read(run_key, app_id), 'autostart must be opt-in')
            self.assertEqual(read(product_key, 'InstallDir'), str(install))
            self.assertEqual(read(uninstall_key, 'UninstallString'),
                             '"' + str(install / 'Uninstall.exe') + '"')
            (install / 'my-notes.txt').write_text('keep my additions')
            (install / 'resources' / 'my-notes.txt').write_text('keep my resources')
            run_setup('/AUTOSTART=1')
            expected = '"' + str(install / 'desktop-pet.exe') + '"'
            self.assertEqual(read(run_key, app_id), expected)
            run_setup()
            self.assertEqual(read(run_key, app_id), expected, 'upgrade lost autostart')
            run_setup('/AUTOSTART=0')
            self.assertIsNone(read(run_key, app_id))
            run_setup('/AUTOSTART=1')
            uninstall()
            self.assertIsNone(read(run_key, app_id))
            self.assertIsNone(read(product_key, 'InstallDir'))
            self.assertIsNone(read(uninstall_key, 'UninstallString'))
            self.assertFalse((install / 'desktop-pet.exe').exists())
            self.assertFalse((install / 'resources' / 'avatar').exists())
            self.assertEqual((install / 'my-notes.txt').read_text(), 'keep my additions')
            self.assertEqual((install / 'resources' / 'my-notes.txt').read_text(),
                             'keep my resources')
        finally:
            uninstall()


if __name__ == '__main__':
    unittest.main()

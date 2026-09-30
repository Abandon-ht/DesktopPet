"""Build one per-user Windows setup EXE containing the verified release package."""
import argparse
from pathlib import Path
import re
import shutil
import subprocess

from archive_package import validate_package


def quoted(value):
    value = str(value)
    if '\n' in value or '\r' in value:
        raise ValueError('installer paths cannot contain newlines')
    return '"' + value.replace('$', '$$').replace('"', '$\\"') + '"'


def script(package, output, version, app_id='DesktopPet', bootstrapper=None, sign=False):
    package = package.resolve()
    files = validate_package(package)
    if not re.fullmatch(r'[A-Za-z0-9-]+', app_id):
        raise ValueError('invalid application ID')
    if not re.fullmatch(r'\d+\.\d+\.\d+', version):
        raise ValueError('version must be major.minor.patch')
    if not (package / 'BUILD-INFO.json').is_file():
        raise ValueError('archive the release package before building the installer')
    definitions = [
        '!define PACKAGE_DIR ' + quoted(package),
        '!define OUTPUT_FILE ' + quoted(output.resolve()),
        '!define APP_ID ' + quoted(app_id),
        '!define APP_VERSION ' + quoted(version),
    ]
    if bootstrapper:
        if not bootstrapper.is_file():
            raise ValueError('WebView2 bootstrapper is missing')
        definitions.append('!define WEBVIEW2_BOOTSTRAPPER ' + quoted(bootstrapper.resolve()))
    if sign:
        if not bootstrapper:
            raise ValueError('a release installer must include the WebView2 bootstrapper')
        signer = Path(__file__).with_name('sign_package.ps1').resolve()
        # NSIS invokes this on its temporary uninstaller before embedding it.
        command = ('"pwsh.exe" -NoProfile -ExecutionPolicy Bypass -File ' + quoted(signer)
                   + ' -PackageDirectory ' + quoted(package)
                   + ' -ExecutableFile "%1" -TrustSelfSignedForVerification')
        definitions.append("!uninstfinalize '" + command + "' = 0")
    install = []
    directories = set()
    for file in files:
        relative = file.relative_to(package)
        parent = str(relative.parent).replace('/', '\\')
        install.append('SetOutPath "$INSTDIR' + ('' if parent == '.' else '\\' + parent.replace('$', '$$')) + '"')
        install.append('File ' + quoted(file))
        directories.update(relative.parents)
    uninstall = ['Delete "$INSTDIR\\' + str(file.relative_to(package)).replace('/', '\\').replace('$', '$$').replace('"', '$\\"') + '"' for file in files]
    for directory in sorted(directories, key=lambda path: (-len(path.parts), str(path))):
        if str(directory) != '.':
            uninstall.append('RMDir "$INSTDIR\\' + str(directory).replace('/', '\\').replace('$', '$$').replace('"', '$\\"') + '"')
    template = Path(__file__).with_name('installer.nsi').read_text(encoding='utf-8')
    return '\n'.join(definitions) + '\n' + template.replace('; @INSTALL_FILES@', '\n'.join(install)).replace('; @UNINSTALL_FILES@', '\n'.join(uninstall))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--package', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--version', default='0.1.0')
    parser.add_argument('--app-id', default='DesktopPet', help='use a separate ID for isolated installer QA')
    parser.add_argument('--webview2-bootstrapper', type=Path)
    parser.add_argument('--sign', action='store_true', help='sign the embedded uninstaller on a hosted CI runner')
    parser.add_argument('--makensis', default=shutil.which('makensis') or r'C:\Program Files (x86)\NSIS\makensis.exe')
    args = parser.parse_args()
    output = args.output.resolve()
    if output.exists() or args.package.resolve() in output.parents:
        parser.error('choose a new setup path outside the package')
    output.parent.mkdir(parents=True, exist_ok=True)
    source = output.with_suffix('.nsi')
    source.write_text(script(args.package, output, args.version, args.app_id, args.webview2_bootstrapper, args.sign), encoding='utf-8-sig')
    subprocess.run([args.makensis, '/V2', str(source)], check=True)
    if not output.is_file():
        raise RuntimeError('NSIS did not create the installer')
    print('Created ' + output.name)


if __name__ == '__main__':
    main()

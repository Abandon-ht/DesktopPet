"""Archive a signed portable package, recording hashes after signing."""
import argparse
import json
from pathlib import Path
import zipfile

from prepare_resources import sha256


def validate_package(package):
    package = package.resolve()
    required = ('desktop-pet.exe', 'avatar-host-2d.exe', 'DesktopPet-signing.cer',
                'resources/avatar/manifest.json', 'start-test.ps1')
    for name in required:
        if not (package / name).is_file():
            raise ValueError('missing package file: ' + name)
    for name in ('resources/dev-data-dir.txt', 'resources/show-settings-on-launch',
                 'resources/voice-settings.json'):
        if (package / name).exists():
            raise ValueError('local QA configuration must not enter a release: ' + name)
    allowed_root = set(required[:3]) | {'start-test.ps1', 'POLICY.md', 'LICENSE', 'ASSETS.md',
                                       'BUILD-INFO.json', 'README.windows.txt'}
    entries = sorted(package.rglob('*'))
    if any(entry.is_symlink() for entry in entries):
        raise ValueError('symbolic links must not enter a release')
    files = [file for file in entries if file.is_file()]
    for file in files:
        relative = file.relative_to(package)
        if file.is_symlink() or (len(relative.parts) == 1 and file.name not in allowed_root):
            raise ValueError('unexpected release file: ' + str(relative))
        if file.suffix.lower() in ('.pfx', '.p12', '.pem', '.key', '.log', '.sqlite', '.sqlite3', '.db') or file.name.upper().startswith('WINDOWS_SIGNING_'):
            raise ValueError('private or runtime file in release: ' + str(relative))
    return files


def archive(package, output, commit):
    package = package.resolve()
    output = output.resolve()
    if output.exists() or package in output.parents:
        raise ValueError('choose a new output ZIP outside the package')
    validate_package(package)
    required = ('desktop-pet.exe', 'avatar-host-2d.exe', 'DesktopPet-signing.cer')
    info = {'commit': commit, 'architecture': 'windows-x64', 'files': {}}
    for name in required[:3]:
        file = package / name
        info['files'][name] = {'bytes': file.stat().st_size, 'sha256': sha256(file)}
    (package / 'BUILD-INFO.json').write_text(json.dumps(info, indent=2) + '\n', encoding='utf-8')
    (package / 'README.windows.txt').write_text(
        'DesktopPet Windows x64 development package\n\n'
        'Extract the complete folder before running desktop-pet.exe. Keep the host and resources beside it.\n'
        'Release startup is silent; use the tray menu to quit.\n'
        'Windows login startup can be enabled or disabled in Startup settings.\n'
        'Use start-test.ps1 when diagnostic logs are needed.\n'
        'Microphone conversation requires configuring an available LLM service and enabling voice in settings.\n'
        'Ollama and its LLM weights are not included. Speech models run on CPU.\n\n'
        'The executables have Authenticode signatures and timestamps. The public signing certificate is included.\n'
        'A self-signed development certificate is not automatically trusted by Windows or SmartScreen.\n'
        'A signature and matching checksums verify integrity; they do not grant Microsoft certification.\n'
        'See POLICY.md and ASSETS.md for asset terms and provenance.\n', encoding='utf-8')
    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=6) as zipped:
        for file in sorted(package.rglob('*')):
            if file.is_file():
                zipped.write(file, (Path(package.name) / file.relative_to(package)).as_posix())
    digest = sha256(output)
    (output.parent / 'SHA256SUMS.txt').write_text(digest + '  ' + output.name + '\n', encoding='utf-8')
    print('Created ' + output.name + ' (SHA-256 ' + digest + ')')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--package', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--commit', required=True)
    args = parser.parse_args()
    archive(args.package, args.output, args.commit)


if __name__ == '__main__':
    main()

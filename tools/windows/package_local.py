"""Assemble a movable Windows test folder from binaries and verified resource ZIPs."""
import argparse
from pathlib import Path
import shutil

from prepare_resources import ARCHIVES, NAMES, ROOT, prepare


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binaries-dir", type=Path, required=True)
    parser.add_argument("--assets-dir", type=Path, default=ARCHIVES)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--with-voice", action="store_true")
    parser.add_argument("--data-dir", type=Path, help="isolated QA data; omit for normal user storage")
    parser.add_argument("--show-settings", action="store_true")
    args = parser.parse_args()
    output = args.output.resolve()
    if output.exists():
        parser.error("output already exists; choose a new directory")
    binaries = [args.binaries_dir / name for name in ("desktop-pet.exe", "avatar-host-2d.exe")]
    if not all(path.is_file() for path in binaries):
        parser.error("both Release EXEs are required")
    names = NAMES if args.with_voice else ("avatar.zip",)
    prepare(args.assets_dir, output / "resources", names)
    if not (output / "resources/avatar/manifest.json").is_file():
        parser.error("resource package is missing avatar/manifest.json")
    for binary in binaries:
        shutil.copy2(binary, output / binary.name)
    (output / "resources/model-path.txt").write_text("avatar/manifest.json\n", encoding="utf-8")
    if args.data_dir:
        (output / "resources/dev-data-dir.txt").write_text(str(args.data_dir.resolve()) + "\n", encoding="utf-8")
    if args.show_settings:
        (output / "resources/show-settings-on-launch").touch()
    for name in ("POLICY.md", "LICENSE"):
        if (ROOT / name).is_file():
            shutil.copy2(ROOT / name, output / name)
    shutil.copy2(ROOT / "docs/ASSETS.md", output / "ASSETS.md")
    # Run from any working directory; inherited stderr captures both processes.
    (output / "start-test.ps1").write_text(
        "$ErrorActionPreference = 'Stop'\n"
        "$taskLog = Join-Path $PSScriptRoot ('desktop-pet-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.log')\n"
        "$taskOutput = [System.IO.Path]::ChangeExtension($taskLog, '.stdout.log')\n"
        "Write-Host ('Log: ' + $taskLog)\n"
        "$taskProcess = Start-Process -FilePath (Join-Path $PSScriptRoot 'desktop-pet.exe') -WorkingDirectory $PSScriptRoot -WindowStyle Hidden -RedirectStandardError $taskLog -RedirectStandardOutput $taskOutput -PassThru\n"
        "Write-Host 'DesktopPet started. Use the tray menu to quit.'\n"
        "$taskProcess.WaitForExit()\n"
        "exit $taskProcess.ExitCode\n", encoding="utf-8")
    print("Windows test folder: " + str(output))


if __name__ == "__main__":
    main()

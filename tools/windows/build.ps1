param(
    [string]$TargetDirectory = (Join-Path $env:LOCALAPPDATA 'DesktopPet\target'),
    [string]$ProxyUrl = ''
)

$ErrorActionPreference = 'Stop'
$taskRepository = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$taskVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path -LiteralPath $taskVswhere)) { throw 'Visual Studio Installer was not found.' }
$taskVsPath = & $taskVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $taskVsPath) { throw 'Microsoft C++ Build Tools were not found.' }
Import-Module (Join-Path $taskVsPath 'Common7\Tools\Microsoft.VisualStudio.DevShell.dll')
Enter-VsDevShell -VsInstallPath $taskVsPath -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64' | Out-Null
$taskCargoBin = if ($env:CARGO_HOME) { Join-Path $env:CARGO_HOME 'bin' } else { Join-Path $env:USERPROFILE '.cargo\bin' }
$env:Path = $taskCargoBin + ';' + $env:Path
$env:CARGO_TARGET_DIR = $TargetDirectory
if ($ProxyUrl) {
    $env:HTTPS_PROXY = $ProxyUrl
    $env:CARGO_HTTP_PROXY = $ProxyUrl
}

Push-Location $taskRepository
try {
    & rustc +1.95.0-x86_64-pc-windows-msvc -vV
    if ($LASTEXITCODE -ne 0) { throw 'The pinned Rust toolchain is not available.' }
    & cargo +1.95.0-x86_64-pc-windows-msvc build --locked --release -p desktop-pet -p avatar-host-2d
    if ($LASTEXITCODE -ne 0) { throw "Cargo build failed with exit code $LASTEXITCODE" }
    Get-Item (Join-Path $TargetDirectory 'release\desktop-pet.exe'), (Join-Path $TargetDirectory 'release\avatar-host-2d.exe')
} finally {
    Pop-Location
}

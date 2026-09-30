param(
    [Parameter(Mandatory = $true)][string]$PackageDirectory,
    [string]$ExecutableFile,
    [switch]$TrustSelfSignedForVerification,
    [string]$TimestampUrl = 'http://timestamp.digicert.com'
)

$ErrorActionPreference = 'Stop'
foreach ($taskName in @('WINDOWS_SIGNING_CERT_PFX', 'WINDOWS_SIGNING_CERT_PASSWORD', 'WINDOWS_SIGNING_CERT_THUMBPRINT')) {
    if (-not [Environment]::GetEnvironmentVariable($taskName)) { throw "Missing signing secret: $taskName" }
}
if ($TrustSelfSignedForVerification -and ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted')) {
    throw 'Temporary certificate trust is allowed only on a disposable GitHub-hosted Actions runner.'
}
$taskPackage = (Resolve-Path -LiteralPath $PackageDirectory).Path
$taskFiles = if ($ExecutableFile) {
    (Resolve-Path -LiteralPath $ExecutableFile).Path
} else { @('desktop-pet.exe', 'avatar-host-2d.exe') | ForEach-Object {
    $taskFile = Join-Path $taskPackage $_
    if (-not (Test-Path -LiteralPath $taskFile -PathType Leaf)) { throw "Missing executable: $_" }
    $taskFile
} }
$taskExpected = $env:WINDOWS_SIGNING_CERT_THUMBPRINT.Trim().ToUpperInvariant()
if ($taskExpected -notmatch '^[0-9A-F]{40}$') { throw 'Invalid signing certificate thumbprint.' }
$taskSdkBin = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
$taskSignTool = Get-ChildItem -LiteralPath $taskSdkBin -Directory |
    Where-Object { $_.Name -match '^10\.' } |
    Sort-Object { [version]$_.Name } -Descending |
    ForEach-Object { Join-Path $_.FullName 'x64\signtool.exe' } |
    Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } |
    Select-Object -First 1
if (-not $taskSignTool) { throw 'Windows SDK x64 SignTool is required.' }
$taskPfx = Join-Path ([System.IO.Path]::GetTempPath()) ('desktoppet-sign-' + [guid]::NewGuid() + '.pfx')
$taskCertificate = $null
$taskImportedRoot = $false
$taskImportedPersonal = $false
try {
    Write-Host 'Importing the signing certificate into the runner personal store.'
    [System.IO.File]::WriteAllBytes($taskPfx, [Convert]::FromBase64String($env:WINDOWS_SIGNING_CERT_PFX))
    $taskPassword = ConvertTo-SecureString $env:WINDOWS_SIGNING_CERT_PASSWORD -AsPlainText -Force
    $taskImportedPersonal = -not (Test-Path -LiteralPath ("Cert:\CurrentUser\My\" + $taskExpected))
    $taskCertificate = Import-PfxCertificate -FilePath $taskPfx -CertStoreLocation 'Cert:\CurrentUser\My' -Password $taskPassword
    if ($taskCertificate.Thumbprint -ne $taskExpected -or -not $taskCertificate.HasPrivateKey) {
        throw 'The PFX must contain the expected signing certificate and private key.'
    }
    if ($TrustSelfSignedForVerification -and $taskCertificate.Subject -eq $taskCertificate.Issuer) {
        # CurrentUser Root can show a confirmation dialog that blocks headless CI.
        # Hosted Windows runners are administrators and are discarded after the job.
        $taskRoot = [System.Security.Cryptography.X509Certificates.X509Store]::new('Root', 'LocalMachine')
        $taskRoot.Open('ReadWrite')
        try {
            if (-not ($taskRoot.Certificates | Where-Object Thumbprint -EQ $taskExpected)) {
                # Public certificate only; never copy the private key to Root.
                Write-Host 'Temporarily trusting the public certificate on the disposable hosted runner.'
                $taskPublicCertificate = [System.Security.Cryptography.X509Certificates.X509Certificate2]::new($taskCertificate.RawData)
                try { $taskRoot.Add($taskPublicCertificate) } finally { $taskPublicCertificate.Dispose() }
                $taskImportedRoot = $true
            }
        } finally { $taskRoot.Close() }
    }
    foreach ($taskFile in $taskFiles) {
        Write-Host "Signing and timestamping $(Split-Path -Leaf $taskFile)."
        & $taskSignTool sign /sha1 $taskExpected /s My /fd SHA256 /tr $TimestampUrl /td SHA256 $taskFile
        if ($LASTEXITCODE -ne 0) { throw "Authenticode signing failed: $taskFile" }
        Write-Host "Verifying $(Split-Path -Leaf $taskFile)."
        & $taskSignTool verify /pa $taskFile
        if ($LASTEXITCODE -ne 0) { throw "Authenticode verification failed: $taskFile" }
        $taskSignature = Get-AuthenticodeSignature -LiteralPath $taskFile
        if ($taskSignature.Status -ne 'Valid' -or $taskSignature.SignerCertificate.Thumbprint -ne $taskExpected -or -not $taskSignature.TimeStamperCertificate) {
            throw "Invalid signature, signer or timestamp: $taskFile"
        }
    }
    Export-Certificate -Cert $taskCertificate -FilePath (Join-Path $taskPackage 'DesktopPet-signing.cer') | Out-Null
    Write-Host 'All selected executables signed, timestamped and verified.'
} finally {
    Remove-Item -LiteralPath $taskPfx -Force -ErrorAction SilentlyContinue
    if ($taskImportedRoot) { Remove-Item -LiteralPath ("Cert:\LocalMachine\Root\" + $taskExpected) -Force }
    if ($taskImportedPersonal -and $taskCertificate) { Remove-Item -LiteralPath ("Cert:\CurrentUser\My\" + $taskCertificate.Thumbprint) -Force }
}

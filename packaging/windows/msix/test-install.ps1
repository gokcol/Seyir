# Run only on a disposable Windows runner in Windows PowerShell.
param([string]$PackageDirectory = 'dist/msix')

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$identity = 'Seyir.PackagingTest'
$publisher = 'CN=Seyir Packaging Test'
if (Get-AppxPackage -Name $identity) { throw 'A Seyir packaging test app already exists.' }

$packages = @(Get-ChildItem -LiteralPath $PackageDirectory -Filter '*-TEST-ONLY-x64.msix' -File)
if ($packages.Count -ne 1) { throw 'Expected exactly one test MSIX.' }
$package = $packages[0].FullName
$info = Get-Content -LiteralPath "$package.json" -Raw -Encoding utf8 | ConvertFrom-Json
$hash = (Get-FileHash -LiteralPath $package -Algorithm SHA256).Hash
if ($info.identityMode -cne 'test-only' -or $info.name -cne $identity -or
    $info.publisher -cne $publisher -or $hash -ne $info.sha256) {
    throw 'Only the verified test identity may be installed here.'
}
$programFilesX86 = [Environment]::GetEnvironmentVariable('ProgramFiles(x86)')
$sdk = Get-ChildItem -LiteralPath (Join-Path $programFilesX86 'Windows Kits/10/bin') -Directory |
    Where-Object {
        $_.Name -match '^10\.0\.\d+\.\d+$' -and
        (Test-Path (Join-Path $_.FullName 'x64/signtool.exe'))
    } | Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1
if (-not $sdk) { throw 'The Windows SDK SignTool is required.' }
$signtool = Join-Path $sdk.FullName 'x64/signtool.exe'

$work = Join-Path $env:TEMP ('seyir-msix-test-' + [guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $work
$certificate = $null
try {
    $certificateSettings = @{
        Type = 'CodeSigningCert'
        Subject = $publisher
        KeyExportPolicy = 'NonExportable'
        CertStoreLocation = 'Cert:\CurrentUser\My'
        HashAlgorithm = 'SHA256'
        NotAfter = (Get-Date).AddDays(1)
    }
    $certificate = New-SelfSignedCertificate @certificateSettings
    $publicCert = Join-Path $work 'test.cer'
    $null = Export-Certificate -Cert $certificate -FilePath $publicCert
    $null = Import-Certificate -FilePath $publicCert -CertStoreLocation 'Cert:\LocalMachine\TrustedPeople'
    $signed = Join-Path $work 'signed-test.msix'
    Copy-Item -LiteralPath $package -Destination $signed
    & $signtool sign /fd SHA256 /sha1 $certificate.Thumbprint /s My $signed
    if ($LASTEXITCODE -ne 0) { throw 'Signing the test copy failed.' }
    Add-AppxPackage -Path $signed

    $installed = Get-AppxPackage -Name $identity
    if (-not $installed -or $installed.Version -ne $info.version -or
        -not (Test-Path (Join-Path $installed.InstallLocation 'Seyir.exe'))) {
        throw 'Seyir package installation did not match the test artifact.'
    }
    $manifest = Get-AppxPackageManifest -Package $installed.PackageFullName
    $fileAssociation = $manifest.SelectSingleNode("//*[local-name()='FileTypeAssociation']")
    if ($manifest.Package.Applications.Application.Id -cne 'Seyir' -or
        -not $fileAssociation -or $fileAssociation.GetAttribute('Parameters') -cne '"%1"' -or
        -not ($fileAssociation.SelectNodes(".//*[local-name()='FileType']") | Where-Object { $_.InnerText -eq '.png' })) {
        throw 'The installed package needs a desktop entry and an image association.'
    }

    Remove-AppxPackage -Package $installed.PackageFullName
    if (Get-AppxPackage -Name $identity) { throw 'The test package could not be removed.' }
    if ((Get-FileHash -LiteralPath $package -Algorithm SHA256).Hash -ne $hash) {
        throw 'The unsigned submission artifact was modified.'
    }
    Write-Host 'Verified test package installation, image association manifest, and removal.'
} finally {
    foreach ($installed in @(Get-AppxPackage -Name $identity)) {
        Remove-AppxPackage -Package $installed.PackageFullName -ErrorAction Continue
    }
    if ($certificate) {
        Remove-Item "Cert:\LocalMachine\TrustedPeople\$($certificate.Thumbprint)" -ErrorAction SilentlyContinue
        Remove-Item "Cert:\CurrentUser\My\$($certificate.Thumbprint)" -DeleteKey -ErrorAction SilentlyContinue
    }
    Remove-Item -LiteralPath $work -Recurse -Force
}

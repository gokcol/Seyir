# Build an unsigned Partner Center upload or a separate packaging-test MSIX.
[CmdletBinding()]
param(
    [switch]$TestIdentity,
    [string]$IdentityName,
    [string]$Publisher,
    [string]$PublisherDisplayName,
    [string]$DisplayName = 'Seyir',
    [string]$PackageVersion,
    [string]$OutputDirectory = 'dist/msix'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:OS -ne 'Windows_NT') { throw 'MSIX packaging requires Windows and the Windows SDK.' }

if ($TestIdentity) {
    if ($IdentityName -or $Publisher -or $PublisherDisplayName) {
        throw 'Do not combine Store identity values with -TestIdentity.'
    }
    $IdentityName = 'Seyir.PackagingTest'
    $Publisher = 'CN=Seyir Packaging Test'
    $PublisherDisplayName = 'Seyir Packaging Test'
    $DisplayName = 'Seyir Packaging Test'
} elseif ("$IdentityName $Publisher $PublisherDisplayName" -match '(?i)placeholder|packaging[ .-]?test') {
    throw 'A test or placeholder identity cannot be used for a Store package.'
}

if ($IdentityName -notmatch '^[A-Za-z0-9.-]{3,50}$') {
    throw 'Supply Package/Identity/Name from Partner Center.'
}
if (-not $Publisher.StartsWith('CN=') -or $Publisher.Length -gt 8192) {
    throw 'Supply Package/Identity/Publisher from Partner Center, including CN=.'
}
$null = [System.Security.Cryptography.X509Certificates.X500DistinguishedName]::new($Publisher)
foreach ($value in @($PublisherDisplayName, $DisplayName)) {
    if ([string]::IsNullOrWhiteSpace($value) -or $value.Length -gt 256) {
        throw 'Display and publisher names must contain 1–256 characters.'
    }
}

$repo = Join-Path $PSScriptRoot '../..'
Push-Location $repo
try {
    $versionLine = Select-String -LiteralPath 'Cargo.toml' -Pattern '^version = "([0-9]+\.[0-9]+\.[0-9]+)"$' -List
    if (-not $versionLine) { throw 'The Cargo manifest needs a stable app version.' }
    $appVersion = $versionLine.Matches[0].Groups[1].Value
    $defaultVersion = "$appVersion.0"
    if (-not $PackageVersion) { $PackageVersion = $defaultVersion }
    foreach ($value in @($defaultVersion, $PackageVersion)) {
        if ($value -notmatch '^[1-9][0-9]{0,4}\.(0|[1-9][0-9]{0,4})\.(0|[1-9][0-9]{0,4})\.0$' -or
            @($value.Split('.') | Where-Object { [int]$_ -gt 65535 }).Count -ne 0) {
            throw 'MSIX version must have four parts, each at most 65535, with a nonzero major and revision 0.'
        }
    }
    if ([version]$PackageVersion -lt [version]$defaultVersion) {
        throw "Package version must be at least $defaultVersion."
    }

    $programFilesX86 = [Environment]::GetEnvironmentVariable('ProgramFiles(x86)')
    $sdkRoot = Join-Path $programFilesX86 'Windows Kits/10/bin'
    $sdk = Get-ChildItem -LiteralPath $sdkRoot -Directory | Where-Object {
        $_.Name -match '^10\.0\.\d+\.\d+$' -and
        (Test-Path (Join-Path $_.FullName 'x64/makeappx.exe')) -and
        (Test-Path (Join-Path $_.FullName 'x64/makepri.exe'))
    } | Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1
    if (-not $sdk) { throw 'Install the Windows SDK with MakeAppx and MakePri.' }
    $makeappx = Join-Path $sdk.FullName 'x64/makeappx.exe'
    $makepri = Join-Path $sdk.FullName 'x64/makepri.exe'

    $out = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
    $flavor = if ($TestIdentity) { 'TEST-ONLY' } else { 'store' }
    $name = "Seyir-$PackageVersion-$flavor-x64.msix"
    $destination = Join-Path $out $name
    if (Test-Path -LiteralPath $destination) { throw "Package already exists: $destination." }

    $metadata = & cargo +1.93.1 metadata --locked --no-deps --format-version 1
    if ($LASTEXITCODE -ne 0) { throw 'Could not locate the Cargo target directory.' }
    $target = 'x86_64-pc-windows-msvc'
    $previousFlags = $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS
    try {
        $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = "$previousFlags -C target-feature=+crt-static".Trim()
        & cargo +1.93.1 build --locked --release --target $target --bin seyir
        if ($LASTEXITCODE -ne 0) { throw 'Seyir release build failed.' }
    } finally {
        $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = $previousFlags
    }
    $targetDirectory = ($metadata | ConvertFrom-Json).target_directory
    $exe = Join-Path $targetDirectory "$target/release/seyir.exe"

    $work = Join-Path $out ('.work-' + [guid]::NewGuid().ToString('N'))
    $layout = Join-Path $work 'layout'
    $null = New-Item -ItemType Directory -Path $layout -Force
    try {
        Copy-Item -LiteralPath $exe -Destination (Join-Path $layout 'Seyir.exe')
        Copy-Item -LiteralPath 'packaging/windows/msix/Assets' -Destination $layout -Recurse
        [xml]$manifest = Get-Content -LiteralPath 'packaging/windows/msix/AppxManifest.xml' -Raw -Encoding utf8
        $manifest.Package.Identity.SetAttribute('Name', $IdentityName)
        $manifest.Package.Identity.SetAttribute('Publisher', $Publisher)
        $manifest.Package.Identity.SetAttribute('Version', $PackageVersion)
        $manifest.Package.Properties.DisplayName = $DisplayName
        $manifest.Package.Properties.PublisherDisplayName = $PublisherDisplayName
        $uap = 'http://schemas.microsoft.com/appx/manifest/uap/windows10'
        $manifest.GetElementsByTagName('VisualElements', $uap)[0].SetAttribute('DisplayName', $DisplayName)
        $manifest.Save((Join-Path $layout 'AppxManifest.xml'))

        $config = Join-Path $work 'priconfig.xml'
        & $makepri createconfig /cf $config /dq en-US
        if ($LASTEXITCODE -ne 0) { throw 'MakePri configuration failed.' }
        & $makepri new /pr $layout /cf $config /of (Join-Path $layout 'resources.pri')
        if ($LASTEXITCODE -ne 0) { throw 'MakePri resource indexing failed.' }
        $package = Join-Path $work $name
        & $makeappx pack /d $layout /p $package /h SHA256
        if ($LASTEXITCODE -ne 0) { throw 'MakeAppx packaging or validation failed.' }

        $unpacked = Join-Path $work 'unpacked'
        & $makeappx unpack /p $package /d $unpacked
        if ($LASTEXITCODE -ne 0) { throw 'MakeAppx could not verify the finished package.' }
        if ((Get-FileHash -LiteralPath $exe).Hash -ne
            (Get-FileHash -LiteralPath (Join-Path $unpacked 'Seyir.exe')).Hash) {
            throw 'The MSIX executable differs from the release build.'
        }
        [xml]$roundTrip = Get-Content -LiteralPath (Join-Path $unpacked 'AppxManifest.xml') -Raw -Encoding utf8
        $identity = $roundTrip.Package.Identity
        if ($identity.Name -cne $IdentityName -or $identity.Publisher -cne $Publisher -or
            $identity.Version -cne $PackageVersion -or $identity.ProcessorArchitecture -cne 'x64' -or
            $roundTrip.Package.Properties.DisplayName -cne $DisplayName -or
            $roundTrip.Package.Properties.PublisherDisplayName -cne $PublisherDisplayName) {
            throw 'The MSIX identity or display names differ from the requested values.'
        }
        $app = $roundTrip.Package.Applications.Application
        if ($app.Id -cne 'Seyir' -or $app.Executable -cne 'Seyir.exe' -or
            $roundTrip.GetElementsByTagName('VisualElements', $uap)[0].GetAttribute('DisplayName') -cne $DisplayName) {
            throw 'The MSIX app registration is invalid.'
        }
        $caps = @($roundTrip.SelectNodes("//*[local-name()='Capabilities']/*") | ForEach-Object { $_.GetAttribute('Name') })
        if ($caps.Count -ne 1 -or $caps[0] -cne 'runFullTrust') {
            throw 'The package must declare only the desktop runFullTrust capability.'
        }
        $originalTypes = @($manifest.GetElementsByTagName('FileType', $uap) | ForEach-Object { $_.InnerText })
        $packagedTypes = @($roundTrip.GetElementsByTagName('FileType', $uap) | ForEach-Object { $_.InnerText })
        if ($originalTypes.Count -eq 0 -or ($originalTypes -join ',') -cne ($packagedTypes -join ',')) {
            throw 'The MSIX is missing its image associations.'
        }
        if (Test-Path -LiteralPath (Join-Path $unpacked 'AppxSignature.p7x')) {
            throw 'The Partner Center upload must be unsigned.'
        }
        $digest = (Get-FileHash -LiteralPath $package -Algorithm SHA256).Hash.ToLowerInvariant()
        $sourceCommit = & git rev-parse HEAD
        if ($LASTEXITCODE -ne 0) { throw 'Could not record the package source commit.' }
        "$digest  $name" | Set-Content -LiteralPath "$package.sha256" -Encoding ascii
        [ordered]@{
            identityMode = $(if ($TestIdentity) { 'test-only' } else { 'store' })
            name = $IdentityName
            publisher = $Publisher
            publisherDisplayName = $PublisherDisplayName
            displayName = $DisplayName
            appVersion = $appVersion
            version = $PackageVersion
            architecture = 'x64'
            sourceCommit = $sourceCommit
            sdkVersion = $sdk.Name
            sha256 = $digest
            signed = $false
        } | ConvertTo-Json | Set-Content -LiteralPath "$package.json" -Encoding utf8
        Move-Item -LiteralPath @("$package.sha256", "$package.json", $package) -Destination $out
        Write-Host "Built $destination"
    } finally {
        Remove-Item -LiteralPath $work -Recurse -Force
    }
} finally {
    Pop-Location
}

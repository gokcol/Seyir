[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ExecutablePath,
    [ValidatePattern('^[0-9]+\.[0-9]+\.[0-9]+$')][string]$Version,
    [switch]$Verify
)

$ErrorActionPreference = 'Stop'
if ($Verify -and ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted')) {
    throw 'Run installer verification on a disposable GitHub-hosted runner.'
}
if (-not $Version) {
    $manifest = Join-Path $PSScriptRoot '../../Cargo.toml'
    $versionLine = Select-String -LiteralPath $manifest -Pattern '^version = "([0-9]+\.[0-9]+\.[0-9]+)"$' -List
    $Version = $versionLine.Matches[0].Groups[1].Value
}

$source = (Resolve-Path -LiteralPath $ExecutablePath).Path
$output = (New-Item -ItemType Directory -Path (Join-Path $PSScriptRoot '../../dist') -Force).FullName
$iscc = Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6/ISCC.exe'
if (-not (Test-Path -LiteralPath $iscc)) { throw 'Inno Setup 6 is required.' }
$name = "Seyir-$Version-windows-x64-Setup"
& $iscc "/DAppVersion=$Version" "/DExecutablePath=$source" "/O$output" "/F$name" (Join-Path $PSScriptRoot 'Seyir.iss')
if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed with exit code $LASTEXITCODE." }

if ($Verify) {
    $classes = 'HKCU:\Software\Classes'
    $program = Join-Path $classes 'Seyir.Image.1'
    $application = Join-Path $classes 'Applications\seyir.exe'
    $uninstall = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\io.github.gokcol.seyir_is1'
    $shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Seyir.lnk'
    if ((Test-Path $program) -or (Test-Path $application) -or (Test-Path $uninstall) -or (Test-Path $shortcut)) {
        throw 'The disposable runner already has a Seyir registration.'
    }

    $install = Join-Path $env:RUNNER_TEMP 'Seyir Görsel setup test'
    $exe = Join-Path $install 'seyir.exe'
    $uninstaller = Join-Path $install 'unins000.exe'
    $png = Join-Path $classes '.png'
    $before = (Get-Item -LiteralPath $png -ErrorAction SilentlyContinue)?.GetValue('')
    $runArgs = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART')
    function Run-Checked([string]$path, [string[]]$params) {
        $process = Start-Process -FilePath $path -ArgumentList $params -Wait -PassThru
        if ($process.ExitCode -ne 0) { throw "Installer process returned $($process.ExitCode)." }
    }

    try {
        $installArgs = $runArgs + '/SP-' + ('/DIR="{0}"' -f $install)
        Run-Checked (Join-Path $output "$name.exe") $installArgs
        if ((Get-FileHash $exe).Hash -ne (Get-FileHash $source).Hash) {
            throw 'The installed binary differs from the input binary.'
        }
        if (-not (Test-Path $shortcut) -or -not (Test-Path $uninstall)) {
            throw 'The Start Menu shortcut or uninstall entry is missing.'
        }
        $command = '"{0}" "%1"' -f $exe
        foreach ($handler in @($program, $application)) {
            if ((Get-Item "$handler\shell\open\command").GetValue('') -cne $command) {
                throw 'The Explorer command points to the wrong executable.'
            }
        }
        if ('Seyir.Image.1' -notin (Get-Item "$png\OpenWithProgids").GetValueNames()) {
            throw 'The PNG Open With entry is missing.'
        }
    } finally {
        if (Test-Path $uninstaller) { Run-Checked $uninstaller $runArgs }
    }
    if ((Test-Path $exe) -or (Test-Path $shortcut) -or (Test-Path $uninstall) -or
        (Test-Path $program) -or (Test-Path $application)) {
        throw 'Uninstall did not remove the installed app and Explorer integration.'
    }
    $associations = (Get-Item "$png\OpenWithProgids" -ErrorAction SilentlyContinue)?.GetValueNames()
    $after = (Get-Item -LiteralPath $png -ErrorAction SilentlyContinue)?.GetValue('')
    if ($associations -contains 'Seyir.Image.1' -or $before -cne $after) {
        throw 'Uninstall left the Open With entry or changed the PNG default.'
    }
}

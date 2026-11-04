# Exercise the real package on windows-latest; every failed assertion stops the job.
param([Parameter(Mandatory)][string]$Version)
$ErrorActionPreference = 'Stop'
$install = Join-Path $env:LOCALAPPDATA 'Programs\Dutchman'
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Dutchman Convert.lnk'
$registration = 'HKCU:\Software\Dutchman CAD\Dutchman Convert'
$uninstall = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall'
$baselinePath = [string][Environment]::GetEnvironmentVariable('Path', 'User')
$msi = (Resolve-Path "dist/dutchman-$Version-x86_64-pc-windows-msvc.msi").Path
$settings = Join-Path $env:APPDATA "Dutchman Convert $Version\data"
$logs = Join-Path $env:LOCALAPPDATA 'Dutchman\diagnostics'
foreach ($folder in @($settings, $logs)) {
    New-Item -ItemType Directory -Force $folder | Out-Null
    'Keep this file' | Set-Content (Join-Path $folder 'installer-test.txt')
}
function Assert-True($Condition, [string]$Message) {
    if (!$Condition) { throw $Message }
}
function Run-Msi([string]$Action, [string]$Package, [string]$Log) {
    $logPath = Join-Path (Resolve-Path dist).Path $Log
    $p = Start-Process "$env:SystemRoot\System32\msiexec.exe" -ArgumentList "$Action `"$Package`" /qn /norestart /L*v `"$logPath`"" -Wait -PassThru
    if ($p.ExitCode -ne 0) {
        Get-Content -LiteralPath $logPath | Write-Host
        throw "msiexec $Action failed with $($p.ExitCode)"
    }
}
function Entries {
    @(Get-ChildItem $uninstall -ErrorAction SilentlyContinue | Get-ItemProperty |
        Where-Object { $_.DisplayName -eq 'Dutchman Convert' })
}
function Check-Installed([string]$ExpectedVersion) {
    foreach ($name in @('dutchman-gui.exe', 'dutchman.exe', 'dutchman-mcp.exe', 'LICENSE', 'NOTICE', 'Spectral-OFL.txt', 'IBM-Plex-OFL.txt', 'sysinfo-MIT.txt', 'AGENTS.md', 'skills/dutchman-converting/SKILL.md', 'docs/desktop.md', 'docs/json.md')) {
        Assert-True (Test-Path (Join-Path $install $name)) "Missing installed file: $name"
    }
    foreach ($doc in Get-ChildItem docs -Recurse -File -Filter '*.md') {
        $relative = [IO.Path]::GetRelativePath((Get-Location).Path, $doc.FullName)
        Assert-True (Test-Path (Join-Path $install $relative)) "Missing installed doc: $relative"
    }
    foreach ($skill in Get-ChildItem skills -Recurse -File) {
        $relative = [IO.Path]::GetRelativePath((Get-Location).Path, $skill.FullName)
        Assert-True (Test-Path (Join-Path $install $relative)) "Missing installed skill resource: $relative"
    }
    Assert-True (Test-Path $shortcut) 'Missing Start menu shortcut'
    $link = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcut)
    Assert-True ($link.TargetPath -ieq (Join-Path $install 'dutchman-gui.exe')) 'Wrong shortcut target'
    $path = [Environment]::GetEnvironmentVariable('Path', 'User').Split(';').TrimEnd('\')
    Assert-True (@($path | Where-Object { $_ -ieq $install }).Count -eq 1) 'Expected exactly one user PATH entry'
    Assert-True ((Get-ItemProperty $registration).InstallDir.TrimEnd('\') -ieq $install) 'Missing per-user installer identity'
    $entries = @(Entries)
    Assert-True ($entries.Count -eq 1) 'Expected one per-user uninstall entry'
    Assert-True ($entries[0].DisplayVersion -eq $ExpectedVersion) 'Wrong installed version'
    Assert-True ($entries[0].Publisher -eq 'Dutchman CAD') 'Wrong publisher'
    Assert-True (![string]::IsNullOrWhiteSpace($entries[0].DisplayIcon)) 'Missing Add/Remove Programs icon'
    $icon = ([Environment]::ExpandEnvironmentVariables($entries[0].DisplayIcon) -replace ',\s*-?\d+$', '').Trim('"')
    Assert-True (Test-Path -LiteralPath $icon) 'The Add/Remove Programs icon file is missing'
    $machine = @(Get-ChildItem 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall' | Get-ItemProperty |
        Where-Object { $_.DisplayName -eq 'Dutchman Convert' })
    Assert-True ($machine.Count -eq 0) 'Installer created a machine-wide uninstall entry'
    # Read the new PATH in a new shell, as a newly opened terminal would.
    $output = & pwsh -NoProfile -Command '$env:Path = [Environment]::GetEnvironmentVariable("Path", "Machine") + ";" + [Environment]::GetEnvironmentVariable("Path", "User"); dutchman --version; exit $LASTEXITCODE'
    Assert-True ($LASTEXITCODE -eq 0 -and ($output -match [regex]::Escape($Version))) 'dutchman --version failed in a fresh shell'
}
function Check-Clean {
    Assert-True (!(Test-Path $install)) 'Uninstall left program files or the install folder'
    Assert-True (!(Test-Path $shortcut)) 'Uninstall left the shortcut'
    Assert-True (!(Test-Path $registration)) 'Uninstall left installer registration'
    Assert-True (@(Entries).Count -eq 0) 'Uninstall left an Add/Remove Programs entry'
    $after = [string][Environment]::GetEnvironmentVariable('Path', 'User')
    Assert-True ($after.TrimEnd(';') -eq $baselinePath.TrimEnd(';')) 'Uninstall did not restore the user PATH'
    foreach ($folder in @($settings, $logs)) {
        Assert-True ((Get-Content (Join-Path $folder 'installer-test.txt')) -eq 'Keep this file') 'Uninstall removed settings or logs'
    }
}
Run-Msi '/i' $msi 'msi-install.log'
Check-Installed $Version
Run-Msi '/x' $msi 'msi-uninstall.log'
Check-Clean

# There is no previous release MSI yet. Build a lower version from the same source
# to exercise MajorUpgrade now, without downloading a previous release.
$parts = @($Version.Split('.') | ForEach-Object { [int]$_ })
if ($parts[2] -gt 0) { $parts[2]-- }
elseif ($parts[1] -gt 0) { $parts[1]--; $parts[2] = 65535 }
elseif ($parts[0] -gt 0) { $parts[0]--; $parts[1] = 255; $parts[2] = 65535 }
else { throw 'Cannot build an MSI version lower than 0.0.0' }
$lower = $parts -join '.'
& ./scripts/windows-msi.ps1 -Version $lower -Output 'dist/upgrade-from.msi'
$old = (Resolve-Path 'dist/upgrade-from.msi').Path
Run-Msi '/i' $old 'msi-lower-install.log'
Check-Installed $lower
Run-Msi '/i' $msi 'msi-upgrade.log'
Check-Installed $Version
# Downgrades must fail and leave the current installation intact.
$p = Start-Process "$env:SystemRoot\System32\msiexec.exe" -ArgumentList "/i `"$old`" /qn /norestart" -Wait -PassThru
Assert-True ($p.ExitCode -eq 1603) 'Installing an older MSI should be refused'
Check-Installed $Version
Run-Msi '/x' $msi 'msi-upgrade-uninstall.log'
Check-Clean
# The synthetic upgrade package is never a release asset.
Remove-Item 'dist/upgrade-from.msi', 'dist/upgrade-from.msi.sha256', 'dist/upgrade-from.wixpdb' -ErrorAction SilentlyContinue

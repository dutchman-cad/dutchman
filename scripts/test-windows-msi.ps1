# Exercise the real package on windows-latest; every failed assertion stops the job.
param([Parameter(Mandatory)][string]$Version)
$ErrorActionPreference = 'Stop'
$install = Join-Path $env:LOCALAPPDATA 'Programs\Dutchman'
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Dutchman Convert.lnk'
$registration = 'HKCU:\Software\Dutchman CAD\Dutchman Convert'
# Windows Installer keeps a per-user product's Add/Remove Programs entry under the user's SID
# in its own UserData key, not in the HKCU Uninstall key that non-MSI installers use.
$sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$uninstall = "HKLM:\Software\Microsoft\Windows\CurrentVersion\Installer\UserData\$sid\Products"
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
    @(Get-ChildItem $uninstall -ErrorAction SilentlyContinue |
        ForEach-Object { Get-ItemProperty (Join-Path $_.PSPath 'InstallProperties') -ErrorAction SilentlyContinue } |
        Where-Object { $_ -and $_.DisplayName -eq 'Dutchman Convert' })
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
    # ARPPRODUCTICON is published as ProductIcon on the per-user product key, which Add/Remove
    # Programs reads; InstallProperties has no DisplayIcon for an MSI product.
    $packed = Split-Path $entries[0].PSParentPath -Leaf
    $published = Get-ItemProperty "HKCU:\Software\Microsoft\Installer\Products\$packed" -ErrorAction SilentlyContinue
    $iconValue = if ($published -and $published.ProductIcon) { $published.ProductIcon } else { $entries[0].DisplayIcon }
    Assert-True (![string]::IsNullOrWhiteSpace($iconValue)) 'Missing Add/Remove Programs icon'
    $icon = ([Environment]::ExpandEnvironmentVariables($iconValue) -replace ',\s*-?\d+$', '').Trim('"')
    Assert-True (Test-Path -LiteralPath $icon) 'The Add/Remove Programs icon file is missing'
    # A machine-wide install registers the product under the LocalSystem SID; a per-user one
    # only under the installing user's SID (checked above).
    $machine = @(Get-ChildItem 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Installer\UserData\S-1-5-18\Products' -ErrorAction SilentlyContinue |
        ForEach-Object { Get-ItemProperty (Join-Path $_.PSPath 'InstallProperties') -ErrorAction SilentlyContinue } |
        Where-Object { $_ -and $_.DisplayName -eq 'Dutchman Convert' })
    Assert-True ($machine.Count -eq 0) 'Installer registered a machine-wide product'
    # Windows Installer may also list the product in the HKLM Uninstall key; show it for the log.
    Get-ChildItem 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall' | Get-ItemProperty |
        Where-Object { $_.DisplayName -eq 'Dutchman Convert' } |
        ForEach-Object { Write-Host "HKLM Uninstall entry $($_.PSChildName): WindowsInstaller=$($_.WindowsInstaller) InstallLocation=$($_.InstallLocation)" }
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

# Evidence that the package is per-user and needs no admin rights. The runner account is an
# administrator, so install once more as a new standard user, which cannot elevate silently.
function Invoke-Com($object, [string]$member, [string]$kind, [object[]]$arguments) {
    $object.GetType().InvokeMember($member, [Reflection.BindingFlags]::$kind, $null, $object, $arguments)
}
$windowsInstaller = New-Object -ComObject WindowsInstaller.Installer
$summary = Invoke-Com $windowsInstaller 'SummaryInformation' 'GetProperty' @($msi, 0)
$wordCount = [int](Invoke-Com $summary 'Property' 'GetProperty' @(15))
$database = Invoke-Com $windowsInstaller 'OpenDatabase' 'InvokeMethod' @($msi, 0)
function Msi-Property([string]$Name) {
    $view = Invoke-Com $database 'OpenView' 'InvokeMethod' @("SELECT ``Value`` FROM ``Property`` WHERE ``Property``='$Name'")
    Invoke-Com $view 'Execute' 'InvokeMethod' $null | Out-Null
    $record = Invoke-Com $view 'Fetch' 'InvokeMethod' $null
    if ($record) { Invoke-Com $record 'StringData' 'GetProperty' @(1) } else { '<absent>' }
}
$allUsers = Msi-Property 'ALLUSERS'
Write-Host "EVIDENCE package: ALLUSERS=$allUsers MSIINSTALLPERUSER=$(Msi-Property 'MSIINSTALLPERUSER') WordCount=$wordCount"
Assert-True ($allUsers -eq '<absent>' -or $allUsers -eq '') 'The package sets ALLUSERS, so it may install per-machine'
Assert-True (($wordCount -band 8) -ne 0) 'The package does not declare that it needs no elevation'
$database = $null; $summary = $null

$user = 'dutchmanmsitest'
$password = ConvertTo-SecureString ('Dm1!' + [guid]::NewGuid().ToString('N')) -AsPlainText -Force
New-LocalUser -Name $user -Password $password -PasswordNeverExpires | Out-Null
try {
    $sid = (Get-LocalUser $user).SID.Value
    $groups = @(Get-LocalGroup | Where-Object { Get-LocalGroupMember $_ -ErrorAction SilentlyContinue | Where-Object { $_.SID.Value -eq $sid } } | ForEach-Object Name)
    Write-Host "EVIDENCE standard user groups: $($groups -join ', ')"
    Assert-True ($groups -notcontains 'Administrators') 'The test user must not be an administrator'
    $credential = New-Object System.Management.Automation.PSCredential(".\$user", $password)
    $dist = (Resolve-Path dist).Path
    icacls $dist /grant "${user}:(OI)(CI)M" /T | Out-Null
    function Run-MsiAsUser([string]$Action, [string]$Log) {
        $logPath = Join-Path $dist $Log
        $p = Start-Process "$env:SystemRoot\System32\msiexec.exe" -Credential $credential -LoadUserProfile `
            -WorkingDirectory $env:SystemRoot -ArgumentList "$Action `"$msi`" /qn /norestart /L*v `"$logPath`"" -Wait -PassThru
        Write-Host "EVIDENCE standard user msiexec $Action exit $($p.ExitCode)"
        Assert-True ($p.ExitCode -eq 0) "msiexec $Action as a standard user failed with $($p.ExitCode)"
    }
    Run-MsiAsUser '/i' 'msi-standard-user-install.log'
    $userProfile = (Get-CimInstance Win32_UserProfile | Where-Object SID -eq $sid).LocalPath
    $userInstall = Join-Path $userProfile 'AppData\Local\Programs\Dutchman'
    Assert-True (Test-Path (Join-Path $userInstall 'dutchman-gui.exe')) 'The standard user install did not put the app in their LocalAppData'
    $userProducts = "HKLM:\Software\Microsoft\Windows\CurrentVersion\Installer\UserData\$sid\Products"
    $userEntries = @(Get-ChildItem $userProducts -ErrorAction SilentlyContinue |
        ForEach-Object { Get-ItemProperty (Join-Path $_.PSPath 'InstallProperties') -ErrorAction SilentlyContinue } |
        Where-Object { $_ -and $_.DisplayName -eq 'Dutchman Convert' })
    Assert-True ($userEntries.Count -eq 1) 'The standard user install is not registered for that user'
    $systemEntries = @(Get-ChildItem 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Installer\UserData\S-1-5-18\Products' -ErrorAction SilentlyContinue |
        ForEach-Object { Get-ItemProperty (Join-Path $_.PSPath 'InstallProperties') -ErrorAction SilentlyContinue } |
        Where-Object { $_ -and $_.DisplayName -eq 'Dutchman Convert' })
    Assert-True ($systemEntries.Count -eq 0) 'The standard user install registered a machine-wide product'
    $hklmEntries = @(Get-ChildItem 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall' | Get-ItemProperty |
        Where-Object { $_.DisplayName -eq 'Dutchman Convert' })
    Write-Host "EVIDENCE standard user install: HKLM Uninstall entries $($hklmEntries.Count) $($hklmEntries | ForEach-Object { $_.PSChildName })"
    Run-MsiAsUser '/x' 'msi-standard-user-uninstall.log'
    Assert-True (!(Test-Path $userInstall)) 'The standard user uninstall left program files'
    $hklmAfter = @(Get-ChildItem 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall' | Get-ItemProperty |
        Where-Object { $_.DisplayName -eq 'Dutchman Convert' })
    Write-Host "EVIDENCE standard user uninstall: HKLM Uninstall entries $($hklmAfter.Count)"
    Assert-True ($hklmAfter.Count -eq 0) 'The standard user uninstall left an HKLM Uninstall entry'
} finally {
    Remove-LocalUser -Name $user -ErrorAction SilentlyContinue
}

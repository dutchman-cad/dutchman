# WiX 4.0.6 is installed by release-build.yml; it is a build tool only.
param(
    [Parameter(Mandatory)][string]$Version,
    [string]$BinDir = 'target/x86_64-pc-windows-msvc/release',
    [string]$Icon = 'dist/Dutchman.ico',
    [string]$Output
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "MSI needs a stable three-part version: $Version" }
$parts = $Version.Split('.')
if ([int]$parts[0] -gt 255 -or [int]$parts[1] -gt 255 -or [int]$parts[2] -gt 65535) {
    throw "Version exceeds Windows Installer limits: $Version"
}
if (!$Output) { $Output = "dist/dutchman-$Version-x86_64-pc-windows-msvc.msi" }
New-Item -ItemType Directory -Force (Split-Path -Parent $Output) | Out-Null
# Regenerate a small fragment so all bundled guidance is owned and removed by MSI.
$guides = Join-Path (Split-Path -Parent $Output) 'installer-guides'
$fragment = Join-Path (Split-Path -Parent $Output) 'installer-guides.wxs'
if (Test-Path $guides) { Remove-Item -Recurse -Force $guides }
if (Test-Path Env:DUTCHMAN_ELECTRONICS_DIR) {
    Copy-Item -LiteralPath "$env:DUTCHMAN_ELECTRONICS_DIR/dutchman-electronics.exe" -Destination $BinDir
    Copy-Item -Recurse -LiteralPath "$env:DUTCHMAN_ELECTRONICS_DIR/THIRD-PARTY" -Destination $BinDir
} else {
    & bash "$root/scripts/fetch-electronics.sh" x86_64-pc-windows-msvc $BinDir
    if ($LASTEXITCODE -ne 0) { throw 'Could not stage the electronics module' }
}
& python scripts/package-docs.py $guides --wix $fragment --extra-notices "$BinDir/THIRD-PARTY"
if ($LASTEXITCODE -ne 0) { throw 'Could not stage the installed guides and module notices' }
& wix build assets/windows/dutchman.wxs $fragment -arch x64 -d "Version=$Version" -d "BinDir=$BinDir" -d "Icon=$Icon" -o $Output
if ($LASTEXITCODE -ne 0) { throw "WiX build failed: $LASTEXITCODE" }
$hash = (Get-FileHash -LiteralPath $Output -Algorithm SHA256).Hash.ToLowerInvariant()
"$hash  $([IO.Path]::GetFileName($Output))" | Set-Content -LiteralPath "$Output.sha256" -Encoding ascii

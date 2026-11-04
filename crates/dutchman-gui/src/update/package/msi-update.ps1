param(
    [uint32]$ParentProcessId,
    [string]$InstallerPath,
    [string]$GuiPath,
    [string]$LogPath,
    [string]$ResultPath,
    [string]$Tag
)
$ErrorActionPreference = 'Stop'
$code = 1
try {
    Wait-Process -Id $ParentProcessId -Timeout 120 -ErrorAction SilentlyContinue
    if (Get-Process -Id $ParentProcessId -ErrorAction SilentlyContinue) {
        throw 'Dutchman did not close within two minutes. Close it before updating.'
    }
    # Start-Process joins arguments: quote each path explicitly, including paths with spaces.
    $arguments = "/i `"$InstallerPath`" /qb /norestart /L*v `"$LogPath`""
    $process = Start-Process -FilePath "$env:SystemRoot\System32\msiexec.exe" -ArgumentList $arguments -PassThru
    if (!$process.WaitForExit(900000)) {
        $process.Kill()
        throw 'The Windows update timed out after 15 minutes. Open the release page to retry.'
    }
    $code = $process.ExitCode
} catch {
    $_.ToString() | Out-File -LiteralPath $LogPath -Append -Encoding utf8
} finally {
    $result = @{ tag = $Tag; code = $code } | ConvertTo-Json -Compress
    [IO.File]::WriteAllText($ResultPath, $result, [Text.UTF8Encoding]::new($false))
    Remove-Item -LiteralPath (Split-Path -Parent $InstallerPath) -Recurse -Force -ErrorAction SilentlyContinue
    if (!(Get-Process -Id $ParentProcessId -ErrorAction SilentlyContinue)) {
        Start-Process -FilePath $GuiPath
    }
}

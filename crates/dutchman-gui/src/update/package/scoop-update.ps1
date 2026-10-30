param(
    [uint32]$ParentProcessId,
    [string]$ScoopPath,
    [string]$GuiPath,
    [string]$LogPath,
    [string]$Tag
)
$ErrorActionPreference = 'Stop'
$code = 1
$job = $null
try {
    Wait-Process -Id $ParentProcessId -Timeout 120 -ErrorAction SilentlyContinue
    if (Get-Process -Id $ParentProcessId -ErrorAction SilentlyContinue) {
        throw 'Dutchman did not close within two minutes. Close it before updating with Scoop.'
    }
    $job = Start-Job -ArgumentList $ScoopPath, $LogPath -ScriptBlock {
        param($ScoopPath, $LogPath)
        $ErrorActionPreference = 'Stop'
        foreach ($step in @('refresh', 'upgrade')) {
            $arguments = @('update')
            if ($step -eq 'upgrade') { $arguments += 'dutchman' }
            "Running Scoop $($arguments -join ' ')" | Out-File -LiteralPath $LogPath -Append -Encoding utf8
            if ([IO.Path]::GetExtension($ScoopPath) -ieq '.ps1') {
                & powershell.exe -NoProfile -NonInteractive -File $ScoopPath @arguments *>&1 |
                    Out-File -LiteralPath $LogPath -Append -Encoding utf8
            } else {
                & $ScoopPath @arguments *>&1 | Out-File -LiteralPath $LogPath -Append -Encoding utf8
            }
            $exitCode = $LASTEXITCODE
            if ($exitCode -ne 0) { return [int]$exitCode }
        }
        return 0
    }
    if (Wait-Job -Job $job -Timeout 900) {
        $result = Receive-Job -Job $job -ErrorAction Stop
        if ($job.State -ne 'Completed' -or $null -eq $result) {
            throw 'The Scoop update worker stopped before reporting its result.'
        }
        $code = [int]$result
    } else {
        'Scoop update timed out after 15 minutes. Copy the command to retry in PowerShell.' |
            Out-File -LiteralPath $LogPath -Append -Encoding utf8
        $code = 124
    }
} catch {
    $_.ToString() | Out-File -LiteralPath $LogPath -Append -Encoding utf8
    $code = 1
} finally {
    try {
        if ($null -ne $job) {
            Stop-Job -Job $job -ErrorAction SilentlyContinue
            Remove-Job -Job $job -Force -ErrorAction SilentlyContinue
        }
        "DUTCHMAN_SCOOP_RESULT $code $Tag" | Out-File -LiteralPath $LogPath -Append -Encoding utf8
    } finally {
        Start-Process -FilePath $GuiPath
    }
}

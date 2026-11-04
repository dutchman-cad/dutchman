param([uint32]$ParentProcessId, [string]$GuiPath)
$ErrorActionPreference = 'Stop'
Wait-Process -Id $ParentProcessId -Timeout 120 -ErrorAction SilentlyContinue
if (!(Get-Process -Id $ParentProcessId -ErrorAction SilentlyContinue)) {
    Start-Process -FilePath $GuiPath
}

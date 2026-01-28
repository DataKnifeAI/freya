# Freya - Entry point for Windows (PowerShell).
# Run from repo root, same as Linux runs "make" from root:
#   .\make.ps1 help
#   .\make.ps1 up
#   .\make.ps1 llm-pull -Model dolphin3
# Delegates to scripts\windows\freya.ps1. See docs/installation.md.

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$WindowsScript = Join-Path $Root "scripts\windows\freya.ps1"

if (-not (Test-Path $WindowsScript)) {
    Write-Host "Freya Windows script not found: $WindowsScript" -ForegroundColor Red
    Write-Host "Run this from the Freya repo root (where docker-compose.yml is)." -ForegroundColor Yellow
    exit 1
}

& $WindowsScript @args
exit $LASTEXITCODE

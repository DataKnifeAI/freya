# Check Freya dependencies on Windows (Docker, Docker Compose, Git).
# NVIDIA: Install driver for WSL2; use WSL2 + Linux for GPU in containers.
# Run from repo root: .\scripts\windows\check-deps.ps1

$ErrorActionPreference = "Stop"
$Failed = $false
$InstallDoc = "docs/installation.md"

Write-Host "=== Checking Freya dependencies (Windows) ===" -ForegroundColor Cyan
Write-Host ""

# Docker
try {
    $dockerVersion = docker --version 2>$null
    if ($LASTEXITCODE -ne 0) { throw "not found" }
    Write-Host "OK Docker found: $dockerVersion" -ForegroundColor Green
} catch {
    Write-Host "X Docker is not installed or not in PATH." -ForegroundColor Red
    Write-Host "  Install: Docker Desktop for Windows (WSL2 backend)"
    Write-Host "  https://docs.docker.com/desktop/setup/install/windows-install/"
    Write-Host "  See $InstallDoc"
    $Failed = $true
}

# Docker Compose v2
try {
    $composeVersion = docker compose version 2>$null
    if ($LASTEXITCODE -ne 0) { throw "not found" }
    Write-Host "OK Docker Compose found" -ForegroundColor Green
} catch {
    Write-Host "X Docker Compose v2 is not available." -ForegroundColor Red
    Write-Host "  Install Docker Desktop (includes Compose v2)"
    Write-Host "  See $InstallDoc"
    $Failed = $true
}

# Git
try {
    $gitVersion = git --version 2>$null
    if ($LASTEXITCODE -ne 0) { throw "not found" }
    Write-Host "OK Git found: $gitVersion" -ForegroundColor Green
} catch {
    Write-Host "X Git is not installed or not in PATH." -ForegroundColor Red
    Write-Host "  Install: https://git-scm.com/download/win"
    Write-Host "  Or use WSL2 and install git inside the Linux distro."
    Write-Host "  See $InstallDoc"
    $Failed = $true
}

Write-Host ""
if ($Failed) {
    Write-Host "Install missing dependencies above, then run: .\make.ps1 setup" -ForegroundColor Yellow
    Write-Host "Full instructions: $InstallDoc"
    exit 1
}
Write-Host "All required dependencies are present." -ForegroundColor Green
exit 0

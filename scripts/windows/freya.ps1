# Freya - Makefile-like commands for Windows (PowerShell).
# Run from repo root: .\make.ps1 <target> [options]  (same as "make" on Linux)
# Or: .\scripts\windows\freya.ps1 <target> [options]
# Example: .\make.ps1 setup   .\make.ps1 llm-pull -Model dolphin3
# See: make help (on Linux/WSL) or docs/installation.md

param(
    [Parameter(Position = 0)]
    [string]$Target = "help",
    [string]$Model = "",
    [string]$Type = "",
    [string]$Url = "",
    [string]$File = ""
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Split-Path -Parent (Split-Path -Parent $ScriptDir)

function EnsureRepoRoot {
    if (-not (Test-Path "$RepoRoot\docker-compose.yml")) {
        Write-Host "Run from the Freya repo root: .\make.ps1 <target>" -ForegroundColor Red
        exit 1
    }
    Set-Location $RepoRoot
}

function Run-DockerCompose {
    param([string[]]$Args)
    & docker compose @Args
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

function Show-Help {
    Write-Host @"
Freya targets (Windows - from repo root, like 'make' on Linux)
  .\make.ps1 <target>
  help                 Show this help
  check-deps           Check Docker, Docker Compose, Git
  setup                Create directory structure (runs check-deps first)
  build                Build all Docker images
  up                   Start all services
  down                 Stop all services
  sui                  SwarmUI + Ollama only
  cui                  ComfyUI only
  llm                  Ollama only
  llm-pull -Model X    Pull Ollama model (e.g. dolphin3)
  llm-list             List Ollama models
  llm-rm -Model X      Remove Ollama model
  llm-logs             Ollama logs
  logs                 All logs
  status               Service status and URLs
  restart              Restart services
  clean                Stop and remove volumes

Example (from repo root):
  .\make.ps1 setup
  .\make.ps1 build
  .\make.ps1 up
  .\make.ps1 llm-pull -Model dolphin3
"@
}

EnsureRepoRoot

switch ($Target.ToLower()) {
    "help" {
        Show-Help
        exit 0
    }
    "check-deps" {
        & "$ScriptDir\check-deps.ps1"
        exit $LASTEXITCODE
    }
    "setup" {
        & "$ScriptDir\check-deps.ps1"
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        Write-Host "Creating directory structure..."
        $dirs = @(
            "comfyui\models\checkpoints", "comfyui\models\loras", "comfyui\models\vae",
            "comfyui\models\controlnet", "comfyui\models\upscale_models", "comfyui\models\embeddings",
            "comfyui\models\clip", "comfyui\models\clip_vision", "comfyui\models\style_models", "comfyui\models\unet",
            "comfyui\output", "comfyui\input",
            "swarmui\data\Models", "swarmui\data\Models\diffusion_models", "swarmui\data\Models\Stable-Diffusion",
            "swarmui\data\Inputs", "swarmui\output", "swarmui\dlbackend", "swarmui\swarmui-models",
            "swarmui\extensions", "swarmui\bin",
            "ollama\models"
        )
        foreach ($d in $dirs) {
            New-Item -ItemType Directory -Force -Path $d | Out-Null
        }
        Write-Host "Directory structure created." -ForegroundColor Green
        Write-Host "Next: .\make.ps1 build then .\make.ps1 up"
        exit 0
    }
    "build" { Run-DockerCompose -Args @("build"); exit 0 }
    "build-comfyui" { Run-DockerCompose -Args @("build", "comfyui"); exit 0 }
    "build-swarmui" { Run-DockerCompose -Args @("build", "swarmui"); exit 0 }
    "build-no-cache" { Run-DockerCompose -Args @("build", "--no-cache"); exit 0 }
    "up" { Run-DockerCompose -Args @("up", "-d"); exit 0 }
    "down" { Run-DockerCompose -Args @("down"); exit 0 }
    "sui" {
        Run-DockerCompose -Args @("down")
        Run-DockerCompose -Args @("up", "-d", "swarmui", "ollama")
        exit 0
    }
    "cui" {
        Run-DockerCompose -Args @("down")
        Run-DockerCompose -Args @("up", "-d", "comfyui")
        exit 0
    }
    "llm" {
        Run-DockerCompose -Args @("down")
        Run-DockerCompose -Args @("up", "-d", "ollama")
        exit 0
    }
    "restart" { Run-DockerCompose -Args @("restart"); exit 0 }
    "logs" { Run-DockerCompose -Args @("logs", "-f"); exit 0 }
    "logs-comfyui" { Run-DockerCompose -Args @("logs", "-f", "comfyui"); exit 0 }
    "logs-swarmui" { Run-DockerCompose -Args @("logs", "-f", "swarmui"); exit 0 }
    "llm-logs" { Run-DockerCompose -Args @("logs", "-f", "ollama"); exit 0 }
    "clean" { Run-DockerCompose -Args @("down", "-v"); exit 0 }
    "ps" { Run-DockerCompose -Args @("ps"); exit 0 }
    "status" {
        Run-DockerCompose -Args @("ps")
        Write-Host "`n=== Access URLs ===" -ForegroundColor Cyan
        Write-Host "ComfyUI:    http://localhost:8188"
        Write-Host "SwarmUI:    http://localhost:7801"
        Write-Host "Ollama API: http://localhost:11434"
        exit 0
    }
    "llm-pull" {
        if (-not $Model) {
            Write-Host "Usage: .\make.ps1 llm-pull -Model <name>" -ForegroundColor Red
            Write-Host "Example: .\make.ps1 llm-pull -Model dolphin3"
            exit 1
        }
        Write-Host "Pulling Ollama model: $Model"
        try {
            docker compose exec ollama ollama pull $Model
        } catch {
            docker compose run --rm ollama ollama pull $Model
        }
        exit 0
    }
    "llm-list" {
        Write-Host "=== Available models ==="
        Write-Host "Browse: https://ollama.com/library"
        Write-Host "Default: dolphin3 - https://ollama.com/library/dolphin3"
        Write-Host "`n=== Installed models ==="
        try {
            docker compose exec ollama ollama list
        } catch {
            docker compose run --rm ollama ollama list
        }
        if ($LASTEXITCODE -ne 0) { Write-Host "Start Ollama first: .\make.ps1 llm" }
        exit 0
    }
    "llm-rm" {
        if (-not $Model) {
            Write-Host "Usage: .\make.ps1 llm-rm -Model <name>" -ForegroundColor Red
            exit 1
        }
        Write-Host "Removing Ollama model: $Model"
        try {
            docker compose exec ollama ollama rm $Model
        } catch {
            docker compose run --rm ollama ollama rm $Model
        }
        exit 0
    }
    "sui-rebuild" {
        Run-DockerCompose -Args @("down")
        Run-DockerCompose -Args @("up", "-d", "swarmui", "ollama")
        Start-Sleep -Seconds 5
        docker compose exec swarmui bash -c "cd /app/SwarmUI && (test -x ./launchtools/update-linuxmac.sh && ./launchtools/update-linuxmac.sh || (cd src && dotnet build SwarmUI.csproj -c Release))"
        Write-Host "Then run: .\make.ps1 down ; .\make.ps1 sui"
        exit 0
    }
    "swarmui-rebuild" {
        docker compose exec swarmui bash -c "cd /app/SwarmUI && (test -x ./launchtools/update-linuxmac.sh && ./launchtools/update-linuxmac.sh || (cd src && dotnet build SwarmUI.csproj -c Release))"
        Write-Host "Then run: .\make.ps1 down ; .\make.ps1 sui"
        exit 0
    }
    default {
        Write-Host "Unknown target: $Target" -ForegroundColor Red
        Show-Help
        exit 1
    }
}

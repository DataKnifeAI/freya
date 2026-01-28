# Architecture & Project Structure

## Project Structure

```
freya/
├── Dockerfile.comfyui
├── Dockerfile.swarmui
│   ├── Dockerfile.comfyui  # ComfyUI build from source
│   └── Dockerfile.swarmui  # SwarmUI build from source
├── docker-compose.yml      # Docker Compose configuration
├── k8s/                    # Kubernetes manifests
│   ├── comfyui/           # ComfyUI Kubernetes resources
│   ├── swarmui/          # SwarmUI Kubernetes resources
│   └── ingress.yaml      # Ingress configuration
├── comfyui/                # ComfyUI data directories
│   ├── models/           # Model files (mounted as volume)
│   ├── output/           # Generated images
│   └── input/            # Input images
├── swarmui/               # SwarmUI data directories
│   ├── data/             # SwarmUI Data (Models, Inputs, settings)
│   │   ├── Models/       # Model files
│   │   └── Inputs/       # Input images
│   ├── output/           # Generated images (Output path)
│   ├── dlbackend/        # ComfyUI backend installation (persistent)
│   └── swarmui-models/   # SwarmUI internal model storage (persistent)
├── ollama/                # Ollama models (gitignored)
├── docs/                  # Documentation
├── scripts/               # Utility scripts
└── Makefile               # Build automation
```

## Docker Compose Services

### ComfyUI

- **Build**: Custom Dockerfile building from source
- **Source**: https://github.com/comfyanonymous/ComfyUI
- **Port**: 8188
- **Volumes**: 
  - `./comfyui/models` → Model storage
  - `./comfyui/output` → Generated images
  - `./comfyui/input` → Input images

### SwarmUI

- **Build**: Custom Dockerfile building from source
- **Source**: https://github.com/mcmonkeyprojects/SwarmUI
- **Port**: 7801
- **Python**: 3.12 with pip support
  - **Volumes**:
  - `./swarmui/data` → Data (Models, Inputs, settings, databases)
  - `./swarmui/output` → Generated images (Output path)
  - `./swarmui/dlbackend` → ComfyUI backend installation (persistent)
  - `./swarmui/swarmui-models` → SwarmUI internal model storage (persistent)
  - `./swarmui/extensions` → SwarmUI Extensions source (e.g. MagicPrompt)
  - `./swarmui/bin` → SwarmUI build output (persisted so recompiled app with extensions is used after restart)

### Ollama

- **Image**: `ollama/ollama:latest`
- **Port**: 11434 (API)
- **Volumes**: `./ollama/models` → `~/.ollama` (model storage)
- **Role**: Local LLM API; use inside SwarmUI via the [MagicPrompt extension](https://github.com/HartsyAI/SwarmUI-MagicPromptExtension) for prompt enhancement, captions, and LLM-backed generation. See [SwarmUI MagicPrompt](swarmui-magicprompt.md).

## Building Images

### Build All Images
```bash
make build
```

### Build Individual Images
```bash
make build-comfyui
make build-swarmui
```

### Build Without Cache
```bash
make build-no-cache
```

**Note**: First build may take 15-30 minutes as it clones repositories and installs dependencies.

# Architecture & Project Structure

## Project Structure

```
freya/
├── dockerfiles/
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
│   ├── models/           # Model files (mounted as volume)
│   ├── output/           # Generated images
│   ├── input/            # Input images
│   ├── data/             # SwarmUI data directory
│   ├── dlbackend/        # ComfyUI backend installation (persistent)
│   └── swarmui-models/   # SwarmUI internal model storage (persistent)
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
  - `./swarmui/models` → Model storage (Data/Models)
  - `./swarmui/output` → Generated images (Data/Outputs)
  - `./swarmui/input` → Input images (Data/Inputs)
  - `./swarmui/data` → SwarmUI data directory (settings, databases)
  - `./swarmui/dlbackend` → ComfyUI backend installation (persistent)
  - `./swarmui/swarmui-models` → SwarmUI internal model storage (persistent)

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

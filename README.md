# Freya - ComfyUI & SwarmUI Stable Diffusion Platform

A Kubernetes-ready platform for running ComfyUI and SwarmUI Stable Diffusion interfaces with discrete GPU support. Both UIs are built from source using custom Docker images.

## Features

- 🎨 **ComfyUI**: Advanced node-based workflow interface for Stable Diffusion (built from source)
- 🐝 **SwarmUI**: Modular AI image and video generation web interface (built from source)
- 🐳 **Docker Support**: Custom-built containers with GPU support for local development
- ☸️ **Kubernetes Ready**: Designed to run in Kubernetes clusters
- 🚀 **GPU Accelerated**: Full CUDA support for discrete GPUs
- 🔧 **Custom Builds**: All images built from source, not using public images

## Requirements

### System Requirements

- **OS**: Linux (Ubuntu 22.04+ recommended, Arch Linux/CachyOS tested)
- **Docker**: Docker Engine 20.10+ and Docker Compose v2.0+
- **GPU**: NVIDIA GPU with CUDA support (CUDA 12.1+)
- **GPU Memory**: Minimum 8GB VRAM (16GB+ recommended for both services)
- **RAM**: Minimum 16GB system RAM (32GB+ recommended)
- **Storage**: Minimum 50GB free space for models and images
- **Network**: Internet connection for downloading models and dependencies

### Software Requirements

- **NVIDIA Container Toolkit**: Required for GPU access in containers
- **Git**: For cloning repositories during Docker builds
- **Python 3.12**: Installed in SwarmUI container (handled automatically)
- **.NET 8 SDK**: Installed in SwarmUI container (handled automatically)
- **CUDA 12.1**: Provided by base Docker images

### Prerequisites

- Docker and Docker Compose (v3.8+)
- NVIDIA GPU with CUDA support
- NVIDIA Container Toolkit installed
- At least 8GB GPU memory recommended
- Git (for cloning repositories during build)

### Installing NVIDIA Container Toolkit

```bash
# Ubuntu/Debian
distribution=$(. /etc/os-release;echo $ID$VERSION_ID)
curl -s -L https://nvidia.github.io/nvidia-docker/gpgkey | sudo apt-key add -
curl -s -L https://nvidia.github.io/nvidia-docker/$distribution/nvidia-docker.list | sudo tee /etc/apt/sources.list.d/nvidia-docker.list
sudo apt-get update && sudo apt-get install -y nvidia-container-toolkit
sudo systemctl restart docker
```

## Quick Start

### Local Development with Docker Compose

1. **Setup directories and download models:**
   ```bash
   make setup              # Create directory structure
   make quick-setup       # Interactive setup with model downloads
   ```

2. **Build images:**
   ```bash
   make build
   ```

3. **Start services:**
   ```bash
   make up
   ```

4. **Access the interfaces:**
   - ComfyUI: http://localhost:8188
   - SwarmUI: http://localhost:7801

5. **Check GPU availability:**
   ```bash
   make check-gpu-comfyui
   make check-gpu-swarmui
   ```

### Using Makefile Commands

```bash
make help              # Show all available commands
make build             # Build all Docker images
make build-comfyui     # Build ComfyUI image only
make build-swarmui     # Build SwarmUI image only
make build-no-cache     # Build without cache
make up                # Start all services
make down              # Stop all services
make logs               # View logs from all services
make logs-comfyui      # View ComfyUI logs only
make logs-swarmui      # View SwarmUI logs only
make restart           # Restart services
make status            # Show service status and URLs
make clean             # Remove containers and volumes
make check-gpu-comfyui # Verify GPU access in ComfyUI
make check-gpu-swarmui # Verify GPU access in SwarmUI
make setup             # Create directory structure
make quick-setup       # Interactive setup with model downloads
make download-model    # Download a model (see MODELS.md)
```

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

## Kubernetes Deployment

The project includes Kubernetes manifests for deploying ComfyUI and SwarmUI to a Kubernetes cluster.

### Prerequisites
- Kubernetes cluster with GPU nodes
- NVIDIA Device Plugin installed
- Persistent volumes configured
- Container registry to push images (or use local images)

### Build and Push Images

```bash
# Build images
make build

# Tag for your registry
docker tag freya-comfyui:latest your-registry/freya-comfyui:latest
docker tag freya-swarmui:latest your-registry/freya-swarmui:latest

# Push to registry
docker push your-registry/freya-comfyui:latest
docker push your-registry/freya-swarmui:latest
```

### Update Kubernetes Manifests

Edit `k8s/comfyui/deployment.yaml` and `k8s/swarmui/deployment.yaml` to use your registry:

```yaml
image: your-registry/freya-comfyui:latest
imagePullPolicy: Always
```

### Deploy to Kubernetes

```bash
# Create namespace
kubectl create namespace freya

# Deploy ComfyUI
kubectl apply -f k8s/comfyui/

# Deploy SwarmUI
kubectl apply -f k8s/swarmui/

# Optional: Deploy ingress
kubectl apply -f k8s/ingress.yaml

# Check status
kubectl get pods -n freya
kubectl get svc -n freya
```

### Access Services

After deployment, access services via:
- Ingress (if configured): Update `k8s/ingress.yaml` with your domain
- Port forwarding: 
  ```bash
  kubectl port-forward -n freya svc/comfyui 8188:8188
  kubectl port-forward -n freya svc/swarmui 7801:7801
  ```
- NodePort/LoadBalancer (modify service type in manifests)

## Environment Variables

Both services support GPU configuration via environment variables:
- `NVIDIA_VISIBLE_DEVICES`: GPU device IDs (default: `all`)
- `NVIDIA_DRIVER_CAPABILITIES`: Driver capabilities (default: `compute,utility`)

## Model Management

See [MODELS.md](MODELS.md) for comprehensive model management guide.

### Quick Setup

```bash
# Create directory structure
make setup

# Interactive setup with model downloads
make quick-setup

# Download individual models
make download-model TYPE=checkpoint URL=https://... FILE=model.safetensors
```

### Model Directories

**ComfyUI:**
- Checkpoints: `./comfyui/models/checkpoints/`
- LoRA: `./comfyui/models/loras/`
- ControlNet: `./comfyui/models/controlnet/`
- VAE: `./comfyui/models/vae/`
- See [MODELS.md](MODELS.md) for full structure

**SwarmUI:**
- Models: `./swarmui/models/`
- SwarmUI automatically detects models in this directory

## Troubleshooting

### Build Issues

**ComfyUI build fails:**
- Ensure you have sufficient disk space (build requires ~10GB)
- Check internet connection (clones from GitHub)
- Verify CUDA base image compatibility

**SwarmUI build fails:**
- Ensure .NET 8 SDK installs correctly
- Check for sufficient memory during build
- Verify Git access to SwarmUI repository
- Verify Python 3.12 and pip installation in Dockerfile

### GPU Not Detected

**Symptoms:**
- `nvidia-smi` works on host but not in containers
- Error: `could not select device driver "" with capabilities: [[gpu]]`
- `docker info` shows no NVIDIA runtime

**Solution - Install NVIDIA Container Toolkit:**

```bash
# Ubuntu/Debian
distribution=$(. /etc/os-release;echo $ID$VERSION_ID)
curl -s -L https://nvidia.github.io/nvidia-docker/gpgkey | sudo apt-key add -
curl -s -L https://nvidia.github.io/nvidia-docker/$distribution/nvidia-docker.list | sudo tee /etc/apt/sources.list.d/nvidia-docker.list
sudo apt-get update && sudo apt-get install -y nvidia-container-toolkit
sudo systemctl restart docker

# Verify installation
docker run --rm --gpus all nvidia/cuda:12.1.0-base-ubuntu22.04 nvidia-smi
```

**Run diagnostic script:**
```bash
chmod +x scripts/check-gpu.sh
./scripts/check-gpu.sh
```

### Port Conflicts
If ports 8188 or 7801 are already in use, modify `docker-compose.yml`:
```yaml
ports:
  - "8189:8188"  # Change host port
```

### Out of Memory
- Reduce image resolution in the UI
- Use smaller models
- Run only one service at a time
- Increase GPU memory or use multiple GPUs

### First Launch Takes Time
- ComfyUI: Downloads models on first use
- SwarmUI: Initializes database and downloads dependencies
- Both: First generation may be slower as models load into GPU memory

## Notes

- Models are downloaded on first use and cached in the mounted volumes
- Generated images are saved to the respective `output/` directories
- The first model load may take several minutes
- GPU memory usage depends on model size and image resolution
- Both services can run simultaneously if you have sufficient GPU memory (16GB+ recommended)
- Custom builds ensure you have the latest features and full control over dependencies

## License

MIT

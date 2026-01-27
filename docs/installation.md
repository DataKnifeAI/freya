# Installation Guide

## System Requirements

- **OS**: Linux (Ubuntu 22.04+ recommended, Arch Linux/CachyOS tested)
- **Docker**: Docker Engine 20.10+ and Docker Compose v2.0+
- **GPU**: NVIDIA GPU with CUDA support (CUDA 12.4+ or 13.x; images use 13.1.1)
- **GPU Memory**: Minimum 8GB VRAM (16GB+ recommended for both services)
- **RAM**: Minimum 16GB system RAM (32GB+ recommended)
- **Storage**: Minimum 50GB free space for models and images
- **Network**: Internet connection for downloading models and dependencies

## Software Requirements

- **NVIDIA Container Toolkit**: Required for GPU access in containers
- **Git**: For cloning repositories during Docker builds
- **Python 3.12**: Installed in SwarmUI container (handled automatically)
- **.NET 8 SDK**: Installed in SwarmUI container (handled automatically)
- **CUDA 13.1**: Provided by base Docker images (Ubuntu 24.04)

## Installing NVIDIA Container Toolkit

### Ubuntu/Debian

```bash
distribution=$(. /etc/os-release;echo $ID$VERSION_ID)
curl -s -L https://nvidia.github.io/nvidia-docker/gpgkey | sudo apt-key add -
curl -s -L https://nvidia.github.io/nvidia-docker/$distribution/nvidia-docker.list | sudo tee /etc/apt/sources.list.d/nvidia-docker.list
sudo apt-get update && sudo apt-get install -y nvidia-container-toolkit
sudo systemctl restart docker
```

### Verify Installation

```bash
docker run --rm --gpus all nvidia/cuda:13.1.1-base-ubuntu24.04 nvidia-smi
```

## Initial Setup

1. **Clone the repository:**
   ```bash
   git clone https://github.com/DataKnifeAI/freya.git
   cd freya
   ```

2. **Create directory structure:**
   ```bash
   make setup
   ```

3. **Optional: Download models interactively:**
   ```bash
   make quick-setup
   ```

4. **Build Docker images:**
   ```bash
   make build
   ```

5. **Start services:**
   ```bash
   make up
   ```

6. **Access the interfaces:**
   - ComfyUI: http://localhost:8188
   - SwarmUI: http://localhost:7801

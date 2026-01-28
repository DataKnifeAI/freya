# Installation Guide

## Supported Linux distributions

Freya supports Docker Compose on these distributions (NVIDIA Container Toolkit available):

| Distribution        | Versions tested / recommended     |
|---------------------|-----------------------------------|
| **Ubuntu**          | 22.04 LTS, 24.04 LTS              |
| **Debian**          | 12 (Bookworm), 13 (Trixie)        |
| **Fedora**          | 38, 39, 40+                       |
| **RHEL / CentOS**   | RHEL 8/9, CentOS Stream 8/9       |
| **openSUSE**        | Leap 15.x, Tumbleweed             |
| **Arch Linux**      | Rolling (incl. CachyOS, EndeavourOS) |

Install **Docker Engine** and **Docker Compose v2** using your distro’s packages (e.g. `apt`, `dnf`, `zypper`, `pacman`) before following the steps below. See [Docker: Install Docker Engine](https://docs.docker.com/engine/install/) for per-distro instructions.

## System Requirements

- **OS**: Linux (see supported distributions above)
- **Docker**: Docker Engine 20.10+ and Docker Compose v2.0+
- **GPU**: NVIDIA GPU with CUDA support (CUDA 12.4+ or 13.x; images use 13.1.1)
- **GPU Memory**: Minimum 8GB VRAM (16GB+ recommended for both services)
- **RAM**: Minimum 16GB system RAM (32GB+ recommended)
- **Storage**: Minimum 50GB free space for models and images
- **Network**: Internet connection for downloading models and dependencies

## Software Requirements

- **NVIDIA Container Toolkit**: Required for GPU access in containers (install per distro below)
- **Git**: For cloning repositories during Docker builds
- **Python 3.12**: Installed in SwarmUI container (handled automatically)
- **.NET 8 SDK**: Installed in SwarmUI container (handled automatically)
- **CUDA 13.1**: Provided by base Docker images (Ubuntu 24.04)

## Installing NVIDIA Container Toolkit

Install the toolkit for your distribution, then run the [Verify Installation](#verify-installation) command.

### Ubuntu / Debian

```bash
distribution=$(. /etc/os-release;echo $ID$VERSION_ID)
curl -s -L https://nvidia.github.io/nvidia-docker/gpgkey | sudo gpg --dearmor -o /usr/share/keyrings/nvidia-docker.gpg
curl -s -L https://nvidia.github.io/nvidia-docker/$distribution/nvidia-docker.list | \
  sed 's#deb https://#deb [signed-by=/usr/share/keyrings/nvidia-docker.gpg] https://#' | \
  sudo tee /etc/apt/sources.list.d/nvidia-docker.list
sudo apt-get update && sudo apt-get install -y nvidia-container-toolkit
sudo systemctl restart docker
```

### Fedora / RHEL / CentOS

```bash
curl -s -L https://nvidia.github.io/libnvidia-container/stable/rpm/nvidia-container-toolkit.repo | \
  sudo tee /etc/yum.repos.d/nvidia-container-toolkit.repo
sudo dnf install -y nvidia-container-toolkit  # RHEL/CentOS: use yum if dnf not available
sudo nvidia-ctk runtime configure --runtime=docker
sudo systemctl restart docker
```

### openSUSE Leap / Tumbleweed

```bash
# Add repository (e.g. opensuse-leap15.6 or opensuse-tumbleweed)
distribution=$(. /etc/os-release; echo $ID$VERSION_ID)
sudo zypper ar https://nvidia.github.io/nvidia-docker/$distribution/nvidia-docker.repo
sudo zypper ref && sudo zypper in -y nvidia-container-toolkit
sudo nvidia-ctk runtime configure --runtime=docker
sudo systemctl restart docker
```

### Arch Linux (and derivatives: CachyOS, EndeavourOS)

```bash
sudo pacman -S --noconfirm nvidia-container-toolkit
sudo nvidia-ctk runtime configure --runtime=docker
sudo systemctl restart docker
```

Alternatively use the helper script (Arch and Ubuntu/Debian): `sudo ./scripts/install-nvidia-toolkit.sh`

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

3. **Optional: Quick setup** (dirs + ComfyUI starter models via script; add more with `make download-model`; see [model-downloads.md](model-downloads.md)):
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
   - Ollama API: http://localhost:11434 (for SwarmUI MagicPrompt)

7. **Optional — Ollama for MagicPrompt:** Start Ollama and pull the default model ([dolphin3](https://ollama.com/library/dolphin3), recommended for control and compatibility):
   ```bash
   make llm
   make llm-pull MODEL=dolphin3
   ```
   See [SwarmUI MagicPrompt](swarmui-magicprompt.md) for configuration.

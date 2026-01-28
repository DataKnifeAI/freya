# Installation Guide

This guide covers Docker-based setup for Freya (ComfyUI, SwarmUI, Ollama). For security and isolation benefits of containers, see [SwarmUI’s Docker overview](https://github.com/mcmonkeyprojects/SwarmUI/blob/master/docs/Docker.md#why-use-docker).

## Supported platforms

Freya supports **Linux** and **Windows** only (NVIDIA GPU support only; macOS is not supported).

| Platform | Docker setup | GPU support |
|----------|--------------|-------------|
| **Linux** | Docker Engine + Docker Compose v2 | ✅ NVIDIA (via NVIDIA Container Toolkit) |
| **Windows** | PowerShell + Docker Desktop, or WSL2 | ✅ NVIDIA (WSL2 or Docker Desktop) — see [Windows](#windows) |

**Linux** is the primary supported platform (run `make` from repo root; scripts in `scripts/linux/`). **Windows** users run `.\make.ps1` from repo root (PowerShell, same name as `make`) or use WSL2 with Docker Desktop. **macOS** is not supported (no NVIDIA GPU).

---

## Checking and installing Docker

Before installing Freya, ensure Docker and Docker Compose are available.

### Check if Docker is installed

```bash
docker --version
docker compose version
```

You need **Docker Engine 20.10+** and **Docker Compose v2** (the `docker compose` plugin, not the legacy `docker-compose` standalone). If either command is missing or too old, install or upgrade as below.

### Install Docker by platform

- **Linux:** Install [Docker Engine](https://docs.docker.com/engine/install/) using your distro’s package manager (apt, dnf, zypper, pacman). You do **not** need Docker Desktop; the Engine is enough.  
  Then do the [Linux post-install steps](https://docs.docker.com/engine/install/linux-postinstall/) so you can run Docker as a normal user (add your user to the `docker` group and `newgrp docker` or log out and back in).
- **Windows:** Install [Docker Desktop for Windows](https://docs.docker.com/desktop/setup/install/windows-install/) with the **WSL2** backend. Enable “Use the WSL 2 based engine” in Docker Desktop settings. Then use **WSL2** (e.g. Ubuntu from the Microsoft Store) to clone Freya and run `make` / `docker compose`; see [Windows](#windows) below.

After installing, run the check again or use Freya’s dependency checker from the repo root:

```bash
docker --version
docker compose version
# Or: make check-deps   (checks Docker, Docker Compose, Git; tries to install Git on Linux; warns on NVIDIA)
```

---

## Linux

### Supported distributions

Freya supports Docker Compose on these Linux distributions (NVIDIA Container Toolkit available):

| Distribution        | Versions tested / recommended     |
|---------------------|-----------------------------------|
| **Ubuntu**          | 22.04 LTS, 24.04 LTS              |
| **Debian**          | 12 (Bookworm), 13 (Trixie)        |
| **Fedora**          | 38, 39, 40+                       |
| **RHEL / CentOS**   | RHEL 8/9, CentOS Stream 8/9       |
| **openSUSE**        | Leap 15.x, Tumbleweed             |
| **Arch Linux**      | Rolling (incl. CachyOS, EndeavourOS) |

Install **Docker Engine** and **Docker Compose v2** using your distro’s packages, then complete the [post-install steps](https://docs.docker.com/engine/install/linux-postinstall/) (add your user to the `docker` group so you can run Docker without `sudo`).

## System Requirements

- **OS**: Linux (primary) or Windows via WSL2 (see [Supported platforms](#supported-platforms))
- **Docker**: Docker Engine 20.10+ and Docker Compose v2.0+ (see [Checking and installing Docker](#checking-and-installing-docker))
- **GPU**: NVIDIA GPU with CUDA support (CUDA 12.4+ or 13.x; images use 13.1.1). Required for acceleration on Linux and Windows (WSL2).
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

Alternatively use the helper script (Arch and Ubuntu/Debian): `sudo ./scripts/linux/install-nvidia-toolkit.sh`

### Verify Installation

```bash
docker run --rm --gpus all nvidia/cuda:13.1.1-base-ubuntu24.04 nvidia-smi
```

---

## Windows

On Windows you can run Freya in two ways: **PowerShell** (native) or **WSL2** with Docker Desktop. From the repo root, run `.\make.ps1 <target>` (same as `make <target>` on Linux).

1. **Install Docker Desktop for Windows**  
   [Install Docker Desktop on Windows](https://docs.docker.com/desktop/setup/install/windows-install/). Use the **WSL2** backend (default in recent installers). Ensure WSL2 is enabled and you have a Linux distro installed (e.g. Ubuntu from the Microsoft Store).

2. **Install NVIDIA drivers on Windows** (for GPU passthrough to WSL2)  
   Install the latest [NVIDIA driver for Windows](https://www.nvidia.com/Download/index.aspx) that supports [WSL GPU](https://developer.nvidia.com/cuda/wsl). Docker Desktop will use the GPU inside WSL2.

3. **Open a WSL2 terminal** (e.g. “Ubuntu” from the Start menu) and run the same steps as Linux:
   - Install Docker inside WSL2 per [Docker Engine on Linux](https://docs.docker.com/engine/install/) (or use Docker Desktop’s integration so the WSL2 distro uses the host Docker).
   - If using Docker Desktop: it can expose Docker to WSL2; then install [NVIDIA Container Toolkit](https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/install-guide.html) inside the WSL2 distro if you want GPU in containers.
   - Clone Freya, run `make setup`, `make build`, `make up` from the repo directory inside WSL2.

4. **Access the UIs**  
   From Windows, open a browser to http://localhost:8188 (ComfyUI) and http://localhost:7801 (SwarmUI). Docker Desktop forwards ports from WSL2 to Windows.

**PowerShell (same as `make` on Linux):** From the repo root in PowerShell, run:

- **Help / any target:** `.\make.ps1 <target>` — e.g. `.\make.ps1 help`, `.\make.ps1 setup`, `.\make.ps1 build`, `.\make.ps1 up`, `.\make.ps1 llm-pull -Model dolphin3`.
- **Check dependencies only:** `.\scripts\windows\check-deps.ps1` — checks Docker, Docker Compose, and Git (or run `.\make.ps1 check-deps`).

The root `make.ps1` forwards to `scripts\windows\freya.ps1`. Run from the repo root (where `docker-compose.yml` is). GPU in containers on native Windows may require Docker Desktop with WSL2 backend; for full NVIDIA support, use WSL2 as above.

---

## Initial Setup

The steps below work on **Linux** and on **Windows** (inside a WSL2 terminal). Run `make setup` first; it checks Docker, Docker Compose, and Git (and will try to install Git on Linux if missing), then creates directories. See [Checking and installing Docker](#checking-and-installing-docker) if you need to install dependencies.

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

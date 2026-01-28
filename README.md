# Freya - Stable Diffusion with UI on Kubernetes

![Freya](freya.png)

A Kubernetes-ready platform for running ComfyUI and SwarmUI Stable Diffusion interfaces with discrete GPU support. Both UIs are built from source using custom Docker images.

## Features

- 🎨 **ComfyUI**: Advanced node-based workflow interface for Stable Diffusion (built from source)
- 🐝 **SwarmUI**: Modular AI image and video generation web interface (built from source)
- 💬 **Open WebUI + Ollama**: Stable Diffusion prompt generator agent (chat UI backed by local LLMs)
- 🐳 **Docker Support**: Custom-built containers with GPU support for local development
- ☸️ **Kubernetes Ready**: Designed to run in Kubernetes clusters
- 🚀 **GPU Accelerated**: Full CUDA support for discrete GPUs
- 🔧 **Custom Builds**: All images built from source, not using public images

## Quick Start

### Prerequisites

- Linux (Ubuntu 22.04+ recommended)
- Docker & Docker Compose v2.0+
- NVIDIA GPU with 8GB+ VRAM (16GB+ recommended)
- NVIDIA Container Toolkit installed

See [Installation Guide](docs/installation.md) for detailed setup instructions.

### Getting Started

```bash
# Setup directories
make setup

# Optional: Quick setup = dirs + ComfyUI starter models (SDXL + VAE); add more via make download-model
make quick-setup

# Build images
make build

# Start services
make up
```

### Access the Interfaces

- **ComfyUI**: http://localhost:8188
- **SwarmUI**: http://localhost:7801
- **Open WebUI** (SD prompt generator): http://localhost:8080

## Makefile Commands

```bash
make help              # Show all available commands
make build             # Build all Docker images
make up                # Start all services
make down              # Stop all services
make logs              # View logs from all services
make restart           # Restart services
make status            # Show service status and URLs
make check-gpu-comfyui # Verify GPU access in ComfyUI
make check-gpu-swarmui # Verify GPU access in SwarmUI
make setup-ollama MODEL=llama3.2:1b  # Pull an Ollama model for Open WebUI
make logs-open-webui   # View Open WebUI logs
```

See [Architecture Guide](docs/architecture.md) for complete command reference.

## Documentation

- [Installation Guide](docs/installation.md) - System requirements and setup
- [GPU Compatibility](docs/gpu-compatibility.md) - Compatible GPU cards
- [Kubernetes Deployment](docs/kubernetes.md) - Deploy to Kubernetes
- [Model Management](docs/MODELS.md) - Download and manage models
- [SwarmUI Video Guide](docs/swarmui-video-guide.md) - Beginner’s guide to T2V/I2V with SwarmUI (links to [HF guide](https://huggingface.co/blog/MonsterMMORPG/beginners-guide-generate-videos-with-swarmui))
- [Architecture](docs/architecture.md) - Project structure and services
- [Open WebUI prompt agent](docs/open-webui-prompt-agent.md) - Use Open WebUI + Ollama as SD prompt generator
- [Troubleshooting](docs/troubleshooting.md) - Common issues and solutions

## Requirements

- **OS**: Linux (Ubuntu 22.04+ recommended)
- **GPU**: NVIDIA GPU with CUDA 12.4+ or 13.x (images use 13.1.1)
- **VRAM**: Minimum 8GB (16GB+ recommended for both services)
- **RAM**: Minimum 16GB system RAM
- **Storage**: Minimum 50GB free space

See [Installation Guide](docs/installation.md) for detailed requirements.

## Notes

- Models are downloaded on first use and cached in mounted volumes
- Generated images are saved to respective `output/` directories
- First build may take 15-30 minutes
- Both services can run simultaneously with 16GB+ GPU memory

## Acknowledgments

This project was built with [Cursor](https://cursor.sh) using the Composer model.

## License

MIT

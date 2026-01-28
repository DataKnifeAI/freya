#!/bin/bash
# Setup script for Freya - Creates directory structure for models

set -e

echo "Setting up Freya model directories..."

# ComfyUI directories
echo "Creating ComfyUI directories..."
mkdir -p comfyui/models/checkpoints
mkdir -p comfyui/models/loras
mkdir -p comfyui/models/vae
mkdir -p comfyui/models/controlnet
mkdir -p comfyui/models/upscale_models
mkdir -p comfyui/models/embeddings
mkdir -p comfyui/models/clip
mkdir -p comfyui/models/clip_vision
mkdir -p comfyui/models/style_models
mkdir -p comfyui/models/unet
mkdir -p comfyui/output
mkdir -p comfyui/input

# SwarmUI directories (Models and Inputs live under data)
echo "Creating SwarmUI directories..."
mkdir -p swarmui/data/Models
mkdir -p swarmui/data/Models/diffusion_models
mkdir -p swarmui/data/Models/Stable-Diffusion
mkdir -p swarmui/data/Inputs
mkdir -p swarmui/output
mkdir -p swarmui/dlbackend
mkdir -p swarmui/swarmui-models

echo "✓ Directory structure created!"
echo ""
echo "Next steps:"
echo "1. Run 'make download-starter-models' to fetch ComfyUI starter models (SDXL + VAE), or 'make quick-setup' for dirs + starters"
echo "2. Add more ComfyUI models: make download-model TYPE=<type> URL=<url> — see docs/model-downloads.md (Hugging Face, Civitai — account + API key required)"
echo "3. SwarmUI: use its built-in model download utility"
echo "4. Run 'make up' to start the services — docs/MODELS.md for model types and layout"

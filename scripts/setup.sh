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
echo "1. Run 'make quick-setup' to download ComfyUI + SwarmUI video models (Wan + LTX)"
echo "2. Or run 'make download-video-models' for video models only (~20 GB)"
echo "3. See docs/MODELS.md for manual download instructions"
echo "4. Run 'make up' to start the services"

#!/bin/bash
# Setup script for Aphrodite - Creates directory structure for models

set -e

echo "Setting up Aphrodite model directories..."

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

# SwarmUI directories
echo "Creating SwarmUI directories..."
mkdir -p swarmui/models
mkdir -p swarmui/output
mkdir -p swarmui/input
mkdir -p swarmui/data

echo "✓ Directory structure created!"
echo ""
echo "Next steps:"
echo "1. Download models to the appropriate directories"
echo "2. See MODELS.md for model download instructions"
echo "3. Run 'make up' to start the services"

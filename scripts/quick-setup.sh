#!/bin/bash
# Quick setup script - Downloads essential models for getting started
# Non-interactive: Downloads recommended models automatically

set -e

echo "Freya Model Setup"
echo "===================="
echo ""
echo "This script will download essential Stable Diffusion models."
echo "This may take a while depending on your internet connection."
echo ""

# Check if setup.sh was run
if [ ! -d "comfyui/models/checkpoints" ]; then
    echo "Running initial setup..."
    chmod +x scripts/setup.sh
    ./scripts/setup.sh
fi

# Make download scripts executable
chmod +x scripts/download-model.sh
chmod +x scripts/download-video-models.sh

echo "Downloading Stable Diffusion XL checkpoint (6.94 GB) - Most Popular..."
./scripts/download-model.sh checkpoint \
    "https://huggingface.co/stabilityai/stable-diffusion-xl-base-1.0/resolve/main/sd_xl_base_1.0.safetensors" \
    "sd_xl_base_1.0.safetensors"

echo ""
echo "Downloading SDXL VAE (335 MB)..."
./scripts/download-model.sh vae \
    "https://huggingface.co/stabilityai/sdxl-vae/resolve/main/sdxl_vae.safetensors" \
    "sdxl_vae.safetensors"

echo ""
echo "Downloading SwarmUI video models (Wan 2.1 14B fp8 + LTX Video 2B fp8, ~20 GB total)..."
./scripts/download-video-models.sh

echo ""
echo "✓ Setup complete!"
echo ""
echo "Models are located in:"
echo "  - comfyui/models/checkpoints/"
echo "  - comfyui/models/vae/"
echo "  - swarmui/data/Models/diffusion_models/  (Wan video)"
echo "  - swarmui/data/Models/Stable-Diffusion/   (LTX video)"
echo ""
echo "Start the services with: make up"
echo "ComfyUI: http://localhost:8188  |  SwarmUI: http://localhost:7801"

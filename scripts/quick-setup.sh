#!/bin/bash
# Quick setup script - Downloads essential models for getting started

set -e

echo "Aphrodite Model Setup"
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

# Make download script executable
chmod +x scripts/download-model.sh

read -p "Download Stable Diffusion 1.5 checkpoint? (4.27 GB) [y/N] " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    echo "Downloading SD 1.5..."
    ./scripts/download-model.sh checkpoint \
        "https://huggingface.co/runwayml/stable-diffusion-v1-5/resolve/main/v1-5-pruned-emaonly.safetensors" \
        "v1-5-pruned-emaonly.safetensors"
fi

read -p "Download VAE for SD 1.5? (334 MB) [y/N] " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    echo "Downloading VAE..."
    ./scripts/download-model.sh vae \
        "https://huggingface.co/stabilityai/sd-vae-ft-mse-original/resolve/main/vae-ft-mse-840000-ema-pruned.safetensors" \
        "vae-ft-mse-840000-ema-pruned.safetensors"
fi

read -p "Download ControlNet Canny? (1.5 GB) [y/N] " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    echo "Downloading ControlNet..."
    ./scripts/download-model.sh controlnet \
        "https://huggingface.co/lllyasviel/control_v11p_sd15_canny/resolve/main/diffusion_pytorch_model.safetensors" \
        "control_v11p_sd15_canny.safetensors"
fi

echo ""
echo "✓ Setup complete!"
echo ""
echo "Models are located in:"
echo "  - comfyui/models/checkpoints/"
echo "  - comfyui/models/vae/"
echo "  - comfyui/models/controlnet/"
echo ""
echo "Start the services with: make up"
echo "Then access ComfyUI at http://localhost:8188"

#!/bin/bash
# Quick setup: directory structure + ComfyUI starter models (SDXL + VAE).
# ComfyUI has no built-in downloader; this script downloads starters. Use the same
# script for more models (see docs/model-downloads.md for Hugging Face / Civitai).

set -e

echo "Freya Quick Setup"
echo "================="
echo ""

# Run full setup if not done yet
if [ ! -d "comfyui/models/checkpoints" ]; then
    echo "Running initial setup..."
    chmod +x scripts/setup.sh
    ./scripts/setup.sh
else
    echo "Directory structure already present."
fi

echo ""
chmod +x scripts/download-model.sh
./scripts/download-model.sh starter

echo ""
echo "✓ Setup complete!"
echo ""
echo "Add more ComfyUI models: make download-model TYPE=<type> URL=<url> [FILE=<filename>]"
echo "  Types: checkpoint, lora, vae, controlnet, upscale, embedding"
echo "  See docs/model-downloads.md for Hugging Face and Civitai (account + API key required)."
echo ""
echo "SwarmUI: use its built-in model download utility (http://localhost:7801)."
echo ""
echo "Start the services with: make up"

#!/bin/bash
# Download helper for Stable Diffusion models
# Usage: ./scripts/download-model.sh <type> <url> [filename]

set -e

MODEL_TYPE=$1
URL=$2
FILENAME=${3:-$(basename "$URL")}

if [ -z "$MODEL_TYPE" ] || [ -z "$URL" ]; then
    echo "Usage: $0 <type> <url> [filename]"
    echo ""
    echo "Types: checkpoint, lora, vae, controlnet, upscale, embedding"
    echo ""
    echo "Examples:"
    echo "  $0 checkpoint https://huggingface.co/runwayml/stable-diffusion-v1-5/resolve/main/v1-5-pruned-emaonly.safetensors"
    echo "  $0 lora https://civitai.com/api/download/models/12345 custom-lora.safetensors"
    exit 1
fi

case $MODEL_TYPE in
    checkpoint)
        TARGET_DIR="comfyui/models/checkpoints"
        ;;
    lora)
        TARGET_DIR="comfyui/models/loras"
        ;;
    vae)
        TARGET_DIR="comfyui/models/vae"
        ;;
    controlnet)
        TARGET_DIR="comfyui/models/controlnet"
        ;;
    upscale)
        TARGET_DIR="comfyui/models/upscale_models"
        ;;
    embedding)
        TARGET_DIR="comfyui/models/embeddings"
        ;;
    *)
        echo "Unknown model type: $MODEL_TYPE"
        echo "Valid types: checkpoint, lora, vae, controlnet, upscale, embedding"
        exit 1
        ;;
esac

mkdir -p "$TARGET_DIR"
TARGET_PATH="$TARGET_DIR/$FILENAME"

echo "Downloading $MODEL_TYPE model..."
echo "From: $URL"
echo "To: $TARGET_PATH"

if command -v wget &> /dev/null; then
    wget -O "$TARGET_PATH" "$URL"
elif command -v curl &> /dev/null; then
    curl -L -o "$TARGET_PATH" "$URL"
else
    echo "Error: wget or curl required"
    exit 1
fi

echo "✓ Model downloaded successfully!"
echo "  Location: $TARGET_PATH"
echo "  Size: $(du -h "$TARGET_PATH" | cut -f1)"

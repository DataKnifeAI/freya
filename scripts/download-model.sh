#!/bin/bash
# Download helper for Stable Diffusion models
# Usage: ./scripts/download-model.sh <type> <url> [filename]
# Supports Civitai API downloads if CIVITAI_API_KEY is set

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
    echo ""
    echo "Note: For Civitai downloads, set CIVITAI_API_KEY in .env file"
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

# Check if URL is a Civitai API endpoint
if [[ "$URL" == *"civitai.com/api/download/models"* ]] || [[ "$URL" == *"civitai.com/api/v1/models"* ]]; then
    # Load .env if it exists
    if [ -f .env ]; then
        export $(cat .env | grep -v '^#' | xargs)
    fi
    
    if [ -z "$CIVITAI_API_KEY" ]; then
        echo "Warning: CIVITAI_API_KEY not set. Civitai downloads may be rate-limited."
        echo "Add your API key to .env file: CIVITAI_API_KEY=your_key_here"
    else
        echo "Using Civitai API key for download..."
    fi
fi

echo "Downloading $MODEL_TYPE model..."
echo "From: $URL"
echo "To: $TARGET_PATH"

if command -v wget &> /dev/null; then
    if [ -n "$CIVITAI_API_KEY" ] && [[ "$URL" == *"civitai.com"* ]]; then
        wget -O "$TARGET_PATH" --header="Authorization: Bearer $CIVITAI_API_KEY" "$URL"
    else
        wget -O "$TARGET_PATH" "$URL"
    fi
elif command -v curl &> /dev/null; then
    if [ -n "$CIVITAI_API_KEY" ] && [[ "$URL" == *"civitai.com"* ]]; then
        curl -L -H "Authorization: Bearer $CIVITAI_API_KEY" -o "$TARGET_PATH" "$URL"
    else
        curl -L -o "$TARGET_PATH" "$URL"
    fi
else
    echo "Error: wget or curl required"
    exit 1
fi

echo "✓ Model downloaded successfully!"
echo "  Location: $TARGET_PATH"
echo "  Size: $(du -h "$TARGET_PATH" | cut -f1)"

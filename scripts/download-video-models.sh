#!/bin/bash
# Download Wan 2.1 14B fp8 and LTX Video 2B fp8 for SwarmUI (good for RTX 4090).
# Wan = best quality; LTX = best speed. See: docs/MODELS.md and SwarmUI Video Model Support.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

# Ensure SwarmUI video dirs exist (matches setup.sh)
mkdir -p swarmui/data/Models/diffusion_models
mkdir -p swarmui/data/Models/Stable-Diffusion

download() {
    local url="$1"
    local dest="$2"
    local name="$3"
    if [ -f "$dest" ]; then
        echo "Already present: $dest"
        return 0
    fi
    echo "Downloading $name..."
    if command -v wget &>/dev/null; then
        wget -q --show-progress -O "$dest" "$url"
    elif command -v curl &>/dev/null; then
        curl -# -L -o "$dest" "$url"
    else
        echo "Error: need wget or curl"
        exit 1
    fi
}

# Wan 2.1 AccVideo T2V 14B fp8 – best quality for 4090, ~15 GB
WAN_URL="https://huggingface.co/Kijai/WanVideo_comfy/resolve/main/Wan2_1-AccVideo-T2V-14B_fp8_e4m3fn.safetensors"
WAN_FILE="Wan2_1-AccVideo-T2V-14B_fp8_e4m3fn.safetensors"
download "$WAN_URL" "swarmui/data/Models/diffusion_models/$WAN_FILE" "Wan 2.1 14B fp8 (T2V)"

# LTX Video 2B 0.9.8 distilled fp8 – best speed, ~4.5 GB
LTX_URL="https://huggingface.co/Lightricks/LTX-Video/resolve/main/ltxv-2b-0.9.8-distilled-fp8.safetensors"
LTX_FILE="ltxv-2b-0.9.8-distilled-fp8.safetensors"
download "$LTX_URL" "swarmui/data/Models/Stable-Diffusion/$LTX_FILE" "LTX Video 2B fp8"

echo ""
echo "✓ Video models ready:"
echo "  - swarmui/data/Models/diffusion_models/$WAN_FILE"
echo "  - swarmui/data/Models/Stable-Diffusion/$LTX_FILE"
echo "Start SwarmUI with: make up"
echo "In SwarmUI: pick Wan under Models for T2V; pick LTX for fast T2V/I2V. See docs/MODELS.md"

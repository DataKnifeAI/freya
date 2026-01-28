#!/bin/bash
# ComfyUI model download script — starter models and add-one-by-one.
# ComfyUI has no built-in downloader; use this script or see docs/model-downloads.md for Hugging Face / Civitai.
# Usage:
#   ./download-model.sh starter
#   ./download-model.sh <type> <url> [filename]
# Types: checkpoint, lora, vae, controlnet, upscale, embedding

set -e

BASE="comfyui/models"
declare -A DIRS=(
  [checkpoint]=checkpoints
  [lora]=loras
  [vae]=vae
  [controlnet]=controlnet
  [upscale]=upscale_models
  [embedding]=embeddings
)

download_one() {
  local url="$1"
  local out="$2"
  local header=""
  if [[ -n "${CIVITAI_API_KEY:-}" && "$url" == *"civitai.com"* ]]; then
    header="Authorization: Bearer $CIVITAI_API_KEY"
  elif [[ -n "${HF_TOKEN:-${HUGGING_FACE_HUB_TOKEN:-}}" && "$url" == *"huggingface"* ]]; then
    header="Authorization: Bearer ${HF_TOKEN:-$HUGGING_FACE_HUB_TOKEN}"
  fi
  echo "Downloading: $url -> $out"
  if [[ -n "$header" ]]; then
    curl -fSL -H "$header" -o "$out" "$url"
  else
    curl -fSL -o "$out" "$url"
  fi
}

do_starter() {
  echo "Downloading ComfyUI starter models (SDXL base + VAE, ~7.3 GB)..."
  mkdir -p "$BASE/checkpoints" "$BASE/vae"

  if [[ ! -f "$BASE/checkpoints/sd_xl_base_1.0.safetensors" ]]; then
    download_one \
      "https://huggingface.co/stabilityai/stable-diffusion-xl-base-1.0/resolve/main/sd_xl_base_1.0.safetensors" \
      "$BASE/checkpoints/sd_xl_base_1.0.safetensors"
  else
    echo "Checkpoint already present: $BASE/checkpoints/sd_xl_base_1.0.safetensors"
  fi

  if [[ ! -f "$BASE/vae/sdxl_vae.safetensors" ]]; then
    download_one \
      "https://huggingface.co/stabilityai/sdxl-vae/resolve/main/sdxl_vae.safetensors" \
      "$BASE/vae/sdxl_vae.safetensors"
  else
    echo "VAE already present: $BASE/vae/sdxl_vae.safetensors"
  fi

  echo "Starter models complete. Add more with: ./scripts/download-model.sh <type> <url> [filename]"
  echo "See docs/model-downloads.md for Hugging Face and Civitai (account + API key required)."
}

do_single() {
  local type="$1"
  local url="$2"
  local file="${3:-}"

  if [[ -z "${DIRS[$type]:-}" ]]; then
    echo "Unknown type: $type. Use one of: checkpoint, lora, vae, controlnet, upscale, embedding"
    exit 1
  fi
  local dir="$BASE/${DIRS[$type]}"
  mkdir -p "$dir"

  if [[ -z "$file" ]]; then
    file=$(basename "${url%%\?*}")
  fi
  local out="$dir/$file"
  download_one "$url" "$out"
  echo "Saved: $out"
}

case "${1:-}" in
  starter) do_starter ;;
  "")
    echo "Usage: $0 starter"
    echo "       $0 <type> <url> [filename]"
    echo "Types: checkpoint, lora, vae, controlnet, upscale, embedding"
    echo "See docs/model-downloads.md for Hugging Face and Civitai (account + API key required)."
    exit 1
    ;;
  *)
    if [[ -z "${2:-}" ]]; then
      echo "Usage: $0 <type> <url> [filename]"
      exit 1
    fi
    do_single "$1" "$2" "${3:-}"
    ;;
esac

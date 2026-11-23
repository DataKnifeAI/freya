#!/usr/bin/env bash
# Build/run freya-local-worker with NVIDIA GPU + FREYA_MODELS_DIR bind-mount.
# Replaces the stub container on :8091. FreyaDiffusionWorker externalEndpoint
# (e.g. https://vllm.dataknife.net) should already point at this host.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE="${FREYA_WORKER_IMAGE:-freya-diffusion-worker:local}"
NAME="${FREYA_WORKER_CONTAINER:-freya-local-worker}"
MODELS_DIR="${FREYA_MODELS_DIR:-/mnt/game2/freya/models}"
PORT="${FREYA_WORKER_PORT:-8091}"
TOKEN="${FREYA_WORKER_TOKEN:-}"
KEEP_LOADED="${FREYA_ENGINE_KEEP_LOADED:-0}"

if [[ -z "$TOKEN" && -f "$ROOT/.env" ]]; then
  # shellcheck disable=SC1091
  set -a
  # shellcheck source=/dev/null
  source "$ROOT/.env"
  set +a
  TOKEN="${FREYA_WORKER_TOKEN:-}"
fi

# Empty model trees only — no Freya demo Civitai seed downloads.
mkdir -p "$MODELS_DIR/loras" "$MODELS_DIR/checkpoints" "$MODELS_DIR/.hf-cache"

echo "Building $IMAGE (CUDA Diffusers worker)…"
docker build -f "$ROOT/Dockerfile.worker" -t "$IMAGE" "$ROOT"

if docker inspect "$NAME" >/dev/null 2>&1; then
  echo "Removing existing container $NAME"
  docker rm -f "$NAME" >/dev/null
fi

SWARM_CKPT="${FREYA_SWARMUI_CHECKPOINTS:-$ROOT/swarmui/models/Stable-Diffusion}"

ARGS=(
  run -d --name "$NAME"
  --restart unless-stopped
  --gpus all
  -p "${PORT}:8091"
  -e FREYA_MODELS_DIR=/models
  -e FREYA_MODELS_HOST_DIR="$MODELS_DIR"
  -e FREYA_ENGINE_KEEP_LOADED="$KEEP_LOADED"
  -e NVIDIA_VISIBLE_DEVICES="${NVIDIA_VISIBLE_DEVICES:-all}"
  -e NVIDIA_DRIVER_CAPABILITIES=compute,utility
  -v "${MODELS_DIR}:/models"
)
# Optional read-only SwarmUI checkpoints (legacy store) under checkpoints/swarmui/
if [[ -d "$SWARM_CKPT" ]]; then
  ARGS+=(-v "${SWARM_CKPT}:/models/checkpoints/swarmui:ro")
fi

if [[ -n "$TOKEN" ]]; then
  ARGS+=(-e "FREYA_WORKER_TOKEN=$TOKEN")
fi
if [[ -n "${HF_TOKEN:-}" ]]; then
  ARGS+=(-e "HF_TOKEN=$HF_TOKEN" -e "HUGGING_FACE_HUB_TOKEN=$HF_TOKEN")
fi

ARGS+=("$IMAGE" --port 8091 --models-dir /models)

echo "Starting $NAME on :$PORT (models=$MODELS_DIR)…"
docker "${ARGS[@]}"
echo "Health: curl -sS http://127.0.0.1:${PORT}/health | jq ."
echo "VRAM: on-demand load during generate; unload after unless FREYA_ENGINE_KEEP_LOADED=1"

#!/usr/bin/env bash
# Pull a newer freya-diffusion-worker image and recreate freya-local-worker on the GPU host.
# External workers are not restarted from Studio — run this on the machine that runs the container.
#
# Examples:
#   ./scripts/update-local-worker.sh --image harbor.dataknife.net/library/freya-diffusion-worker:4a17ec9
#   ./scripts/update-local-worker.sh --from-cr freya-gpu --namespace freya
#
# Reads FREYA_WORKER_TOKEN and paths from ~/.config/freya/local-worker.env when present,
# else FREYA_WORKER_TOKEN from the environment (same as setup-local-worker.sh).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE="${FREYA_WORKER_IMAGE:-}"
FROM_CR=""
CR_NAME="freya-gpu"
CR_NS="${FREYA_CR_NAMESPACE:-freya}"
PORT="${FREYA_WORKER_PORT:-8091}"
MODELS_DIR="${FREYA_MODELS_DIR:-}"
NAME="${FREYA_WORKER_CONTAINER:-freya-local-worker}"
KEEP_LOADED="${FREYA_ENGINE_KEEP_LOADED:-0}"
NO_GPU=0
KUBECONFIG="${KUBECONFIG:-}"

usage() {
  cat <<'EOF'
Usage: update-local-worker.sh [options]

  --image IMAGE       Full image ref (repository:tag). Required unless --from-cr.
  --from-cr NAME      Read spec.image from FreyaDiffusionWorker (kubectl).
  --namespace NS      Namespace for --from-cr (default: freya).
  --port PORT         Host port (default 8091).
  --models-dir DIR    Models bind-mount (default from env file or ~/freya/models).
  --name NAME         Container name (default freya-local-worker).
  --no-gpu            Skip --gpus (stub / CPU).
  -h, --help          Show this help.

Token: ~/.config/freya/local-worker.env, repo .env, or FREYA_WORKER_TOKEN in the environment.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --image) IMAGE="${2:-}"; shift 2 ;;
    --from-cr) FROM_CR="${2:-freya-gpu}"; shift 2 ;;
    --namespace) CR_NS="${2:-}"; shift 2 ;;
    --port) PORT="${2:-}"; shift 2 ;;
    --models-dir) MODELS_DIR="${2:-}"; shift 2 ;;
    --name) NAME="${2:-}"; shift 2 ;;
    --no-gpu) NO_GPU=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/freya"
ENV_FILE="$CONFIG_DIR/local-worker.env"
TOKEN="${FREYA_WORKER_TOKEN:-}"

if [[ -f "$ENV_FILE" ]]; then
  # shellcheck disable=SC1090
  set -a
  source "$ENV_FILE"
  set +a
  TOKEN="${FREYA_WORKER_TOKEN:-$TOKEN}"
  PORT="${FREYA_WORKER_PORT:-$PORT}"
  MODELS_DIR="${FREYA_MODELS_DIR:-$MODELS_DIR}"
  NAME="${FREYA_WORKER_CONTAINER:-$NAME}"
  KEEP_LOADED="${FREYA_ENGINE_KEEP_LOADED:-$KEEP_LOADED}"
fi

if [[ -z "$TOKEN" && -f "$ROOT/.env" ]]; then
  # shellcheck disable=SC1091
  set -a
  source "$ROOT/.env"
  set +a
  TOKEN="${FREYA_WORKER_TOKEN:-$TOKEN}"
fi

MODELS_DIR="${MODELS_DIR:-$HOME/freya/models}"

resolve_image_from_cr() {
  local name="$1" ns="$2"
  if ! command -v kubectl >/dev/null 2>&1; then
    echo "error: kubectl required for --from-cr" >&2
    exit 1
  fi
  local json
  json="$(kubectl --request-timeout=15s -n "$ns" get freyadiffusionworkers "$name" -o json)"
  local repo tag
  repo="$(echo "$json" | python3 -c "
import json,sys
o=json.load(sys.stdin)
img=o.get('spec',{}).get('image') or {}
print(img.get('repository') or 'harbor.dataknife.net/library/freya-diffusion-worker')
")"
  tag="$(echo "$json" | python3 -c "
import json,sys
o=json.load(sys.stdin)
img=o.get('spec',{}).get('image') or {}
print(img.get('tag') or '0.1.0')
")"
  echo "${repo}:${tag}"
}

if [[ -n "$FROM_CR" ]]; then
  CR_NAME="$FROM_CR"
  IMAGE="$(resolve_image_from_cr "$CR_NAME" "$CR_NS")"
  echo "Image from FreyaDiffusionWorker/$CR_NAME: $IMAGE"
fi

if [[ -z "$IMAGE" ]]; then
  echo "error: set --image or --from-cr (or FREYA_WORKER_IMAGE)" >&2
  usage >&2
  exit 1
fi

if [[ -z "$TOKEN" ]]; then
  echo "error: FREYA_WORKER_TOKEN required (env file, .env, or environment)" >&2
  exit 1
fi

if ! docker info >/dev/null 2>&1; then
  echo "error: docker not usable" >&2
  exit 1
fi

echo "Pulling $IMAGE …"
docker pull "$IMAGE"

if docker inspect "$NAME" >/dev/null 2>&1; then
  echo "Removing existing container $NAME"
  docker rm -f "$NAME" >/dev/null
fi

mkdir -p "$MODELS_DIR/loras" "$MODELS_DIR/checkpoints" "$MODELS_DIR/.hf-cache"

ARGS=(
  run -d --name "$NAME"
  --restart unless-stopped
  -p "${PORT}:8091"
  -e FREYA_MODELS_DIR=/models
  -e FREYA_MODELS_HOST_DIR="$MODELS_DIR"
  -e FREYA_ENGINE_KEEP_LOADED="$KEEP_LOADED"
  -e "FREYA_WORKER_TOKEN=$TOKEN"
  -v "${MODELS_DIR}:/models"
)

SWARM_CKPT="${FREYA_SWARMUI_CHECKPOINTS:-$ROOT/swarmui/models/Stable-Diffusion}"
if [[ -d "$SWARM_CKPT" ]]; then
  ARGS+=(-v "${SWARM_CKPT}:/models/checkpoints/swarmui:ro")
fi

if [[ "$NO_GPU" -eq 0 ]]; then
  ARGS+=(--gpus all
    -e NVIDIA_VISIBLE_DEVICES="${NVIDIA_VISIBLE_DEVICES:-all}"
    -e NVIDIA_DRIVER_CAPABILITIES=compute,utility)
fi

ARGS+=("$IMAGE" --port 8091 --models-dir /models)

echo "Starting $NAME on 0.0.0.0:${PORT} (image=$IMAGE)…"
docker "${ARGS[@]}"

echo "Health: curl -sS http://127.0.0.1:${PORT}/health"
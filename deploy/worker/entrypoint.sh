#!/usr/bin/env bash
# Start Diffusers engine (8092) then freya-worker (8091).
set -euo pipefail

MODELS_DIR="${FREYA_MODELS_DIR:-/models}"
ENGINE_HOST="${FREYA_ENGINE_HOST:-127.0.0.1}"
ENGINE_PORT="${FREYA_ENGINE_PORT:-8092}"
export FREYA_MODELS_DIR="$MODELS_DIR"
export FREYA_ENGINE_URL="${FREYA_ENGINE_URL:-http://${ENGINE_HOST}:${ENGINE_PORT}}"

# Empty model trees only — do not seed Freya demo / Civitai assets on first run.
mkdir -p "$MODELS_DIR/loras" "$MODELS_DIR/checkpoints"

echo "freya-worker entrypoint: models=$MODELS_DIR engine=$FREYA_ENGINE_URL"

python3 /opt/freya/engine/server.py &
ENGINE_PID=$!

cleanup() {
  kill "$ENGINE_PID" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

# Wait for engine health (up to ~60s)
for _ in $(seq 1 60); do
  if curl -sf "http://${ENGINE_HOST}:${ENGINE_PORT}/health" >/dev/null 2>&1; then
    echo "diffusers engine ready"
    break
  fi
  if ! kill -0 "$ENGINE_PID" 2>/dev/null; then
    echo "diffusers engine exited early" >&2
    wait "$ENGINE_PID" || true
    exit 1
  fi
  sleep 1
done

exec /usr/local/bin/freya-worker "$@"

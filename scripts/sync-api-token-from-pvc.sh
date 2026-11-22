#!/usr/bin/env bash
# Align Kubernetes Secret freya-api-token with the runtime apiToken on the gallery PVC.
# Use when UI "Generate API token" rotated the API but MCP/env clients still 401.
set -euo pipefail

NS="${FREYA_NAMESPACE:-freya}"
PVC="${FREYA_GALLERY_PVC:-freya-gallery}"
SECRET="${FREYA_API_TOKEN_SECRET:-freya-api-token}"
SECRET_KEY="${FREYA_API_TOKEN_SECRET_KEY:-token}"
SECRETS_PATH="${FREYA_SECRETS_PATH:-/data/gallery/.freya-runtime.json}"
POD="freya-token-sync-$$"

cleanup() {
  kubectl -n "$NS" delete pod "$POD" --ignore-not-found --wait=false >/dev/null 2>&1 || true
}
trap cleanup EXIT

kubectl -n "$NS" delete pod "$POD" --ignore-not-found --wait=true >/dev/null 2>&1 || true
kubectl -n "$NS" run "$POD" --restart=Never --image=busybox:1.36 \
  --overrides="$(cat <<EOF
{"spec":{"containers":[{"name":"peek","image":"busybox:1.36","command":["sleep","60"],"volumeMounts":[{"name":"g","mountPath":"/data/gallery"}]}],"volumes":[{"name":"g","persistentVolumeClaim":{"claimName":"$PVC"}}]}}
EOF
)"
kubectl -n "$NS" wait --for=condition=Ready "pod/$POD" --timeout=60s >/dev/null

TMP="$(mktemp -d)"
kubectl -n "$NS" exec "$POD" -- cat "$SECRETS_PATH" >"$TMP/runtime.json"
python3 - "$TMP" "$SECRET_KEY" <<'PY'
import json, os, sys, hashlib
td, key = sys.argv[1], sys.argv[2]
d = json.load(open(os.path.join(td, "runtime.json")))
tok = (d.get("apiToken") or "").strip()
if not tok:
    raise SystemExit("no apiToken in runtime secrets file")
open(os.path.join(td, "token"), "w").write(tok)
print(f"runtime.apiToken len={len(tok)} sha8={hashlib.sha256(tok.encode()).hexdigest()[:8]}")
PY

kubectl -n "$NS" create secret generic "$SECRET" \
  --from-file="${SECRET_KEY}=${TMP}/token" \
  --dry-run=client -o yaml | kubectl apply -f -

rm -rf "$TMP"
echo "Synced Secret ${NS}/${SECRET}. Restart clients that cache the env token:"
echo "  kubectl -n ${NS} rollout restart deploy/freya-mcp"
echo "Studio UI: Worker → Advanced → paste token → Apply token (or Generate, which stores localStorage)."

#!/usr/bin/env bash
# Sync fleet Let's Encrypt wildcard (*.dataknife.net) into kind-freya and point
# ingress-nginx at it — same pattern as gitops-core / Traefik on prd-apps:
#   kube-system/wildcard-dataknife-net-tls  (+ copy into freya for Ingress tls.secretName)
#   --default-ssl-certificate=kube-system/wildcard-dataknife-net-tls
set -euo pipefail

CONTEXT="${KIND_CONTEXT:-kind-freya}"
SRC_KUBECONFIG="${WILDCARD_SRC_KUBECONFIG:-${HOME}/.kube/prd-apps-rke2.yaml}"
SRC_NS="${WILDCARD_SRC_NS:-kube-system}"
SECRET_NAME="wildcard-dataknife-net-tls"
APP_NS="${FREYA_NAMESPACE:-freya}"

if [[ ! -f "$SRC_KUBECONFIG" ]]; then
  echo "missing source kubeconfig: $SRC_KUBECONFIG" >&2
  echo "set WILDCARD_SRC_KUBECONFIG to a cluster that has ${SECRET_NAME}" >&2
  exit 1
fi

kubectl config use-context "$CONTEXT" >/dev/null

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

kubectl --kubeconfig "$SRC_KUBECONFIG" -n "$SRC_NS" get secret "$SECRET_NAME" -o json \
  | python3 -c '
import json, sys
s = json.load(sys.stdin)
md = s.setdefault("metadata", {})
for k in ("uid", "resourceVersion", "creationTimestamp", "managedFields",
          "ownerReferences", "selfLink", "generation"):
    md.pop(k, None)
anns = md.get("annotations") or {}
md["annotations"] = {k: v for k, v in anns.items()
                     if not k.startswith("kubectl.kubernetes.io")}
labels = md.get("labels") or {}
labels["app.kubernetes.io/managed-by"] = "freya-kind-sync"
md["labels"] = labels
md["namespace"] = "kube-system"
md["name"] = "wildcard-dataknife-net-tls"
print(json.dumps(s))
' >"$tmpdir/kube-system.json"

python3 -c '
import json, sys
s = json.load(open(sys.argv[1]))
s["metadata"]["namespace"] = sys.argv[2]
json.dump(s, open(sys.argv[3], "w"))
' "$tmpdir/kube-system.json" "$APP_NS" "$tmpdir/app.json"

kubectl apply -f "$tmpdir/kube-system.json"
kubectl apply -f "$tmpdir/app.json"

# Ensure ingress-nginx uses the fleet default cert (idempotent).
args=$(kubectl -n ingress-nginx get deploy ingress-nginx-controller \
  -o jsonpath='{.spec.template.spec.containers[0].args}' 2>/dev/null || true)
if [[ -n "$args" ]] && ! grep -q "default-ssl-certificate=kube-system/${SECRET_NAME}" <<<"$args"; then
  kubectl -n ingress-nginx patch deployment ingress-nginx-controller --type=json -p="[
    {\"op\":\"add\",\"path\":\"/spec/template/spec/containers/0/args/-\",\"value\":\"--default-ssl-certificate=kube-system/${SECRET_NAME}\"}
  ]"
  kubectl -n ingress-nginx rollout status deploy/ingress-nginx-controller --timeout=120s
fi

# Point any freya-vllm Ingress at the wildcard (replace self-signed).
if kubectl -n "$APP_NS" get ingress freya-vllm >/dev/null 2>&1; then
  kubectl -n "$APP_NS" patch ingress freya-vllm --type=json -p="[
    {\"op\":\"replace\",\"path\":\"/spec/tls/0/secretName\",\"value\":\"${SECRET_NAME}\"}
  ]" 2>/dev/null \
    || kubectl -n "$APP_NS" patch ingress freya-vllm --type=merge -p="{\"spec\":{\"tls\":[{\"hosts\":[\"vllm.dataknife.net\"],\"secretName\":\"${SECRET_NAME}\"}]}}"
fi

echo "OK: ${SECRET_NAME} in kube-system + ${APP_NS}; nginx default-ssl-certificate set"
echo | openssl s_client -connect 127.0.0.1:443 -servername freya.dataknife.net 2>/dev/null \
  | openssl x509 -noout -issuer -subject -ext subjectAltName

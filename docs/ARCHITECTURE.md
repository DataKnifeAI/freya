# Architecture

```
User → Freya UI → Freya API
                     ├─ OpenRouter (chat + images)
                     ├─ Civitai API (search; authenticated downloads)
                     └─ Diffusion worker (in-cluster or remote)
                            └─ /v1/images/generations
                            └─ /v1/assets/install  (LoRAs for UI)
```

## Secrets

| Secret | FreyaStudio / Worker field | Env | Notes |
|--------|----------------------------|-----|-------|
| OpenRouter | `spec.openRouter.secretRef` | `OPENROUTER_API_KEY` | Also editable in UI Settings |
| Civitai | `spec.civitai.secretRef` | `CIVITAI_API_KEY` | UI Settings |
| Hugging Face | `spec.huggingFace.secretRef` | `HF_TOKEN` | optional |
| API bearer | `FreyaStudio.spec.apiTokenSecretRef` | `FREYA_API_TOKEN` | UI sends bearer from Settings (not injected by nginx); required for Ingress |
| Worker bearer | `FreyaDiffusionWorker.spec.authSecretRef` | `FREYA_WORKER_TOKEN` | Injected on worker + API |

Never put API keys in CRD string fields — only Secret references.

## Worker placement

- **external** (recommended) — `status.endpoint` = `spec.externalEndpoint`; API and `FreyaModelAsset` call that URL. Run the worker on the GPU host; do not assume in-cluster `nvidia.com/gpu`. Patch `spec.image` for the desired Harbor tag; roll the container on the GPU host with `./scripts/update-local-worker.sh --from-cr` (Studio cannot restart the URL).
- **incluster** (optional) — operator creates Deployment + models PVC + Service when the cluster actually has GPUs. Changing `spec.image` reconciles the Deployment image and rolls the pod.

How Studio reaches an external worker from pods:

| Form | Example | When |
|------|---------|------|
| Host IP + port | `http://192.168.3.125:8091` | API pods can route to the GPU host LAN |
| FQDN | `https://vllm.dataknife.net` | Ingress / DNS already fronts the worker on this host |
| `localhost` | `http://127.0.0.1:8091` | API with hostNetwork (or non-cluster `make run-api`) on the same machine as the worker |
| Cluster DNS bridge | `http://freya-gpu-host.freya.svc.cluster.local:8091` | Optional ExternalName / Endpoints Service pointing at the host |

# Architecture

```
User / agent
  ├─ Freya UI  → Freya API
  └─ Freya MCP (Streamable HTTP /mcp) → Freya API
                       ├─ OpenRouter (chat + images)
                       ├─ Civitai API (search; authenticated downloads)
                       └─ Diffusion worker (local GPU host via URL — preferred for SD / LoRAs)
                              └─ /v1/images/generations   (model + lora_request)
                              └─ /v1/assets/install         (LoRAs + checkpoints + meta)
                              └─ /v1/loras | /v1/checkpoints
                              └─ /v1/base-model             (active checkpoint selection)
```

## Default deployment story

Freya does **not** require a dedicated cluster or an in-cluster GPU.

**Single-system without k8s:** Docker Compose under [`deploy/compose/`](../deploy/compose/) runs API + UI + worker (+ optional MCP) on one host — see [COMPOSE.md](COMPOSE.md). No operator or CRDs on that path.

| Where | What |
|-------|------|
| **In cluster** (shared k8s is fine) | Operator, `FreyaStudio` (API + UI + MCP), Ingress |
| **Out of cluster** (GPU host) | `freya-local-worker` (`./scripts/run-local-worker.sh` / Diffusers+CUDA) or `make run-worker`; Studio reaches it by **URL** |

Recommended worker CR: [`config/samples/freya_v1alpha1_freyadiffusionworker_external.yaml`](../config/samples/freya_v1alpha1_freyadiffusionworker_external.yaml) (`placement: external` + `externalEndpoint`). Point that URL at the GPU host (LAN IP, FQDN, or a cluster ExternalName/Endpoints bridge). The Worker tab Host/Port (or Advanced full URL) is the same surface for the API’s `FREYA_DIFFUSION_WORKER_URL`.

**kind** is a **dev** path (local ports, hosts override, sample Ingress). Production-shaped is the **same CRs** on a shared cluster plus a local worker URL — not “Freya owns kind forever,” and not “GPU must live in the same cluster.”

**MCP:** FreyaStudio reconciles `{studio}-mcp` Deployment+Service when `spec.mcpEnabled` (default `true`). Ingress Prefix `/mcp` → MCP `:8090` (path preserved; server mounts Streamable HTTP at `/mcp`). Public URL: `https://<ingress.host>/mcp`. Auth: same `apiTokenSecretRef` → `FREYA_API_TOKEN`; `FREYA_API_URL` points at `{studio}-api:8080`.

**Where bases live:** `FREYA_MODELS_DIR` on the **GPU worker host** (`loras/`, `checkpoints/`, `active-base.json`). Civitai `baseModel` is stored in `*.safetensors.meta.json` and used when the UI/API omits an explicit generate `model`. Remote LLM/vLLM text endpoints are often **not** SD-capable — keep multi-base diffusion on the local RTX worker.

**Motion:** `gif` / `video` modalities use OpenRouter’s async `/videos` API (gallery stores `video/mp4` or converted `image/gif`). Local GPU worker can emit multi-frame **GIF** via Diffusers (`/v1/videos/generations`). Still images + LoRAs stay on the Diffusers engine (`FREYA_ENGINE_URL`); VRAM loads on demand and unloads after generate unless `FREYA_ENGINE_KEEP_LOADED=1`.

## Secrets

| Secret | FreyaStudio / Worker field | Env | Notes |
|--------|----------------------------|-----|-------|
| OpenRouter | `spec.openRouter.secretRef` | `OPENROUTER_API_KEY` | Also editable in UI Settings |
| Civitai | `spec.civitai.secretRef` | `CIVITAI_API_KEY` | UI Settings |
| Hugging Face | `spec.huggingFace.secretRef` | `HF_TOKEN` | optional |
| API bearer | `FreyaStudio.spec.apiTokenSecretRef` | `FREYA_API_TOKEN` | UI sends bearer from Settings (not injected by nginx); also injected on MCP; required for Ingress |
| Worker bearer | `FreyaDiffusionWorker.spec.authSecretRef` | `FREYA_WORKER_TOKEN` | Injected on worker + API |

Never put API keys in CRD string fields — only Secret references.

## Worker placement

- **external** (recommended) — `status.endpoint` = `spec.externalEndpoint`; API and `FreyaModelAsset` call that URL. Run the worker on the GPU host; do not assume in-cluster `nvidia.com/gpu`.
- **incluster** (optional) — operator creates Deployment + models PVC + Service when the cluster actually has GPUs.

How Studio reaches an external worker from pods:

| Form | Example | When |
|------|---------|------|
| Host IP + port | `http://192.168.3.125:8091` | API pods can route to the GPU host LAN |
| FQDN | `https://vllm.dataknife.net` | Ingress / DNS already fronts the worker on this host |
| `localhost` | `http://127.0.0.1:8091` | API with hostNetwork (or non-cluster `make run-api`) on the same machine as the worker |
| Cluster DNS bridge | `http://freya-gpu-host.freya.svc.cluster.local:8091` | Optional ExternalName / Endpoints Service pointing at the host |

# Freya

![Freya](freya.png)

Kubernetes operator and approachable NL image studio for [DataKnifeAI](https://dataknife.ai).

Freya removes the old “download GB → open Comfy/Swarm → click forever” loop:

1. **Describe** in natural language (no comma-tag recipes)  
2. Optionally **ask for suggestions**; Freya **plans** and writes the model brief for you  
3. **Generate** via OpenRouter image models **or** a local/remote diffusion worker with LoRAs  
4. Browse the **gallery** and remix  

The previous Compose/ComfyUI/SwarmUI/Ollama stack is preserved under [`legacy/`](legacy/) for reference — it is not required for the new path.

## Guidelines (required reading)

**Freya is a tool.** It helps you plan prompts and call APIs you configure. It does not decide what is legal where you live.

**You are liable.** Generated or downloaded content may be illegal or restricted under your country or state law. You alone are responsible for how you use Freya and what you publish — not Freya, DataKnifeAI, or the operators of an instance.

**Bring your own keys and hardware.** You supply OpenRouter / Civitai / Hugging Face credentials and any local GPU worker. Freya only relays requests you initiate.

**Follow third-party policies.** Freya does not replace those contracts. For any keys you configure and any prompts/assets you send or download:

| Service | Policies |
|---------|----------|
| **OpenRouter** (chat / image / video) | [Terms](https://openrouter.ai/terms) · [Privacy](https://openrouter.ai/privacy) · [Data collection](https://openrouter.ai/docs/guides/privacy/data-collection) · [ZDR](https://openrouter.ai/docs/guides/features/zdr) |
| **Civitai** (search / LoRA install) | [Terms](https://civitai.com/content/tos) · [Privacy](https://civitai.com/content/privacy) · [Content moderation](https://civitai.com/content/moderation) · [Real-people rules](https://civitai.com/content/rules/real-people) |
| **Hugging Face** (optional HF token / Hub downloads) | [Terms](https://huggingface.co/terms-of-service) · [Privacy](https://huggingface.co/privacy) · [Content policy](https://huggingface.co/content-policy) |

Studio **Guidelines** tab shows the full statement. First sign-in requires an acknowledgment (`localStorage` key `freya.guidelinesAck`). **Settings** links there and holds OpenRouter privacy / alignment prefs (ZDR, `data_collection`, provider allow/deny lists, model catalog filters).

## Quick start (local)

**UI mock only** (no keys/API — for layout and flow tweaks):

```bash
make mock-ui          # or: cd web && npm i && npm run dev:mock
```

**Single-system Docker Compose** (no Kubernetes / operator — see [docs/COMPOSE.md](docs/COMPOSE.md)):

```bash
cp deploy/compose/.env.example deploy/compose/.env
# OPENROUTER_API_KEY + FREYA_ADMIN_PASSWORD
make compose-up       # UI :8080  (worker stays on the compose network)
# make compose-up-gpu # NVIDIA
# make compose-up-mcp # + MCP :8090
```

**Full stack (cargo + Vite):**

```bash
cp .env.example .env
# edit .env — set OPENROUTER_API_KEY=sk-or-... (required)
# optional: CIVITAI_API_KEY, FREYA_API_TOKEN, FREYA_WORKER_TOKEN, FREYA_CORS_ORIGINS
set -a && source .env && set +a

make run-api          # :8080
cd web && npm install && npm run dev   # :5173

# GPU worker on this host (recommended; stays out of cluster)
make run-worker       # :8091
# then Worker tab → Host/Port → http://127.0.0.1:8091 (or LAN IP / FQDN from in-cluster Studio)
```

In the UI:

- **Settings** — OpenRouter + Civitai API keys  
- **Worker** — diffusion host/port or full URL (local GPU process, not “must be in kind”)  

Sessions use **Plan / Auto / YOLO** automation (see [docs/LOCAL.md](docs/LOCAL.md)). Featured example prompt matches `freya.png` brand art.

## Cluster

Default story: **Studio + operator + Ingress in k8s** (shared cluster is fine); **GPU worker stays on the physical host** and is reached by URL. kind is a local **dev** path — production-shaped uses the same samples on shared k8s. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

```bash
make manifests install
kubectl apply -k config/default

kubectl create ns freya
kubectl -n freya create secret generic freya-openrouter --from-literal=apiKey="$OPENROUTER_API_KEY"
kubectl -n freya create secret generic freya-civitai --from-literal=apiKey="$CIVITAI_API_KEY"
kubectl -n freya create secret generic freya-api-token --from-literal=token="$(openssl rand -hex 32)"
kubectl -n freya create secret generic freya-worker-token --from-literal=apiKey="$(openssl rand -hex 32)"

# recommended: external worker URL → local / LAN GPU host
kubectl apply -f config/samples/freya_v1alpha1_freyadiffusionworker_external.yaml
# optional: only if the cluster itself has nvidia.com/gpu
# kubectl apply -f config/samples/freya_v1alpha1_freyadiffusionworker_incluster.yaml

kubectl apply -f config/samples/freya_v1alpha1_freyastudio.yaml
```

### Workers: external URL (recommended) or optional in-cluster GPU

| Placement | Spec | Use when |
|-----------|------|----------|
| `external` (**recommended**) | `externalEndpoint: https://host` or `http://ip:8091` | GPU box / `freya-local-worker` on the host; Studio points at it |
| `incluster` (optional) | GPU Deployment + models PVC | Cluster has `nvidia.com/gpu` — not required for Freya |

`FreyaModelAsset` (or UI **Civitai → Install to worker**) pushes LoRAs onto that worker so they show up for local generation.

Local GPU path: **Diffusers** via `./scripts/run-local-worker.sh` / `Dockerfile.worker` (checkpoints + Civitai LoRAs under `FREYA_MODELS_DIR`, on-demand VRAM). CRD label `engine: vllm-omni` still allows swapping to **[vLLM-Omni](https://github.com/vllm-project/vllm-omni)** on the same HTTP surface. Without `FREYA_ENGINE_URL`, freya-worker returns stub PNGs for wiring tests.

## CRDs

- `FreyaStudio` — API + UI + gallery + **MCP** (`mcpEnabled`, default on); OpenRouter + **Civitai** + HF secret refs  
- `FreyaGeneration` — declarative generate job  
- `FreyaStyle` — curated style packs  
- `FreyaDiffusionWorker` — external URL (recommended) or optional in-cluster engine  
- `FreyaModelAsset` — install weights onto a worker  

**MCP (agents):** Streamable HTTP at `https://freya.dataknife.net/mcp` (Ingress `/mcp` → `{studio}-mcp:8090`). Cursor: `{ "url": "https://freya.dataknife.net/mcp" }`. Local: `make run-mcp`.

## Docs

- [docs/CONCEPTS.md](docs/CONCEPTS.md) — map from legacy Freya  
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — backends & secrets  
- [docs/OPENROUTER.md](docs/OPENROUTER.md) — cloud models  
- [docs/LOCAL.md](docs/LOCAL.md) — local runbook  
- [docs/COMPOSE.md](docs/COMPOSE.md) — single-system Docker Compose (no k8s)  

## License

Apache-2.0 — see [LICENSE](LICENSE).

# Freya

![Freya](freya.png)

Kubernetes operator and approachable NL image studio for [DataKnifeAI](https://dataknife.ai).

Freya removes the old “download GB → open Comfy/Swarm → click forever” loop:

1. **Describe** in natural language (no comma-tag recipes)  
2. Optionally **ask for suggestions**; Freya **plans** and writes the model brief for you  
3. **Generate** via OpenRouter image models **or** a local/remote diffusion worker with LoRAs  
4. Browse the **gallery** and remix  

The previous Compose/ComfyUI/SwarmUI/Ollama stack is preserved under [`legacy/`](legacy/) for reference — it is not required for the new path.

## Quick start (local)

**UI mock only** (no keys/API — for layout and flow tweaks):

```bash
make mock-ui          # or: cd web && npm i && npm run dev:mock
```

**Full stack:**

```bash
export OPENROUTER_API_KEY=sk-or-...
# optional — search/install Civitai LoRAs onto a worker
export CIVITAI_API_KEY=...

make run-api          # :8080
cd web && npm install && npm run dev   # :5173

# optional GPU / remote worker stub
make run-worker       # :8091
# then Settings → Diffusion worker URL → http://127.0.0.1:8091
```

In the UI **Settings** tab you can paste:

- **OpenRouter API key** — chat + cloud images  
- **Civitai API key** — search Civitai and **Install to worker** (LoRAs/checkpoints for the UI)  
- **Diffusion worker URL** — in-cluster Service or a remote physical GPU host  

## Cluster

```bash
make manifests install
kubectl apply -k config/default

kubectl create ns freya
kubectl -n freya create secret generic freya-openrouter --from-literal=apiKey="$OPENROUTER_API_KEY"
kubectl -n freya create secret generic freya-civitai --from-literal=apiKey="$CIVITAI_API_KEY"

# remote GPU box
kubectl apply -f config/samples/freya_v1alpha1_freyadiffusionworker_external.yaml
# or in-cluster GPU
# kubectl apply -f config/samples/freya_v1alpha1_freyadiffusionworker_incluster.yaml

kubectl apply -f config/samples/freya_v1alpha1_freyastudio.yaml
```

### Workers: in-cluster or remote physical

| Placement | Spec | Use when |
|-----------|------|----------|
| `inCluster` | GPU Deployment + models PVC | Cluster has `nvidia.com/gpu` |
| `external` | `externalEndpoint: http://host:8091` | Physical box / bare-metal GPU |

`FreyaModelAsset` (or UI **Civitai → Install to worker**) pushes LoRAs onto that worker so they show up for local generation.

Preferred production engine: **[vLLM-Omni](https://github.com/vllm-project/vllm-omni)** (`/v1/images/generations` + LoRA). This repo includes a **freya-worker** stub for wiring tests.

## CRDs

- `FreyaStudio` — API + UI + gallery; OpenRouter + **Civitai** + HF secret refs  
- `FreyaGeneration` — declarative generate job  
- `FreyaStyle` — curated style packs  
- `FreyaDiffusionWorker` — in-cluster or external engine  
- `FreyaModelAsset` — install weights onto a worker  

## Docs

- [docs/CONCEPTS.md](docs/CONCEPTS.md) — map from legacy Freya  
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — backends & secrets  
- [docs/OPENROUTER.md](docs/OPENROUTER.md) — cloud models  
- [docs/LOCAL.md](docs/LOCAL.md) — local runbook  

## License

Apache-2.0 — see [LICENSE](LICENSE).

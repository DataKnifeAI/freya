# Freya

![Freya](freya.png)

Kubernetes operator and approachable NL image studio for [DataKnifeAI](https://dataknife.ai).

Freya removes the old “download GB → open Comfy/Swarm → click forever” loop:

1. **Describe** what you want  
2. Freya **plans** (OpenRouter chat) and **enhances** the prompt  
3. **Generate** via OpenRouter image models **or** a local/remote diffusion worker with LoRAs  
4. Browse the **gallery** and remix  

The previous Compose/ComfyUI/SwarmUI/Ollama stack is preserved under [`legacy/`](legacy/) for reference — it is not required for the new path.

## Quick start (local)

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
make help              # Show all available commands
make build             # Build all Docker images
make up                # Start all services
make down              # Stop all services
make sui               # SwarmUI + Ollama (stops others first; updates SwarmUI)
make cui               # ComfyUI only (stops others first)
make llm               # Ollama only (stops others first)
make swarmui-update    # Pull latest SwarmUI, rebuild, and restart
make swarmui-rebuild   # Rebuild SwarmUI from current source (no git pull)
make logs              # View logs from all services
make restart           # Restart services
make status            # Show service status and URLs
make check-gpu-comfyui # Verify GPU access in ComfyUI
make check-gpu-swarmui # Verify GPU access in SwarmUI
make llm-pull MODEL=dolphin3  # Pull default Ollama model (MagicPrompt); see https://ollama.com/library/dolphin3
make llm-list          # List installed models + browse link
make llm-rm MODEL=dolphin3  # Remove an Ollama model
make llm-logs          # View Ollama logs
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

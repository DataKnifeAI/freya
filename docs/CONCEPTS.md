# Concepts from legacy Freya

Source material lives in [`legacy/`](../legacy/) (Compose, ComfyUI, SwarmUI, Ollama, MagicPrompt docs).

| Old Freya (`legacy/`) | This stack (repo root) |
|-----------|------------|
| MagicPrompt (Ollama) | `/v1/assist` + OpenRouter (or vLLM chat URL) |
| ComfyUI / SwarmUI | Single Freya UI — no node graphs |
| Civitai manual download | Settings **Civitai API key** + search/install onto worker |
| Ollama dolphin3 | OpenRouter chat models + Freya **router** (Llama 3.1 8B-class when Auto) |
| GPU UIs in-cluster | **External** `FreyaDiffusionWorker` URL → local GPU host (recommended); optional in-cluster GPU only if the cluster has one |

### Modality routing (legacy alignment)

Freya’s light **request router** classifies each chat/run/generate ask the way the old stack split work:

| Modality | Legacy | Freya today |
|----------|--------|-------------|
| **chat** | MagicPrompt Chat / `make llm` (dolphin3) | Agent converse / assist — no generate |
| **picture** | ComfyUI stills (`make cui`); Flux for NL, SDXL ecosystem for LoRAs | OpenRouter `/images` or local worker |
| **gif** | SwarmUI **video** output (`gif-hd` / webp) — not still-image | Classified; clear “unsupported” (not silent image gen) |
| **video** | SwarmUI T2V/I2V (`make sui`; Wan / LTX) | Discovery via `GET /v1/models` → `video`; generate not wired yet |

Settings defaults (all **Auto**): Agent `openrouter/auto`, Image `auto` → Flux NL default, Router `auto` → `meta-llama/llama-3.1-8b-instruct`. Pinned models always win over router suggestions.

Automation north star: user provides intent (and optionally API keys); Freya installs LoRAs and generates without folder clicking.

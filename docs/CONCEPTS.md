# Concepts from legacy Freya

Source material lives in [`legacy/`](../legacy/) (Compose, ComfyUI, SwarmUI, Ollama, MagicPrompt docs).

| Old Freya (`legacy/`) | This stack (repo root) |
|-----------|------------|
| MagicPrompt (Ollama) | `/v1/assist` + OpenRouter (or vLLM chat URL) |
| ComfyUI / SwarmUI | Single Freya UI — no node graphs |
| Civitai manual download | Settings **Civitai API key** + search/install onto worker |
| Ollama dolphin3 | OpenRouter chat models |
| GPU UIs in-cluster | Optional `FreyaDiffusionWorker` (vLLM-Omni) in-cluster **or** remote host |

Automation north star: user provides intent (and optionally API keys); Freya installs LoRAs and generates without folder clicking.

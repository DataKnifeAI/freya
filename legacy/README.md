# Legacy Freya (Compose / ComfyUI / SwarmUI / Ollama)

This directory holds the previous Freya stack: custom Docker images, Kubernetes YAML, and Makefile targets for ComfyUI, SwarmUI, and Ollama.

The **current** Freya product lives at the repository root: Rust operator + Freya API/UI over OpenRouter, with optional remote/in-cluster diffusion workers and Civitai LoRA install.

See root [README.md](../README.md) and [docs/CONCEPTS.md](../docs/CONCEPTS.md).

To run the legacy stack from this folder:

```bash
cd legacy
# follow README.compose.md / Makefile help
```

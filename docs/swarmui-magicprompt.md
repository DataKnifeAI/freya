# Using Ollama Inside SwarmUI (MagicPrompt Extension)

You can use **Ollama** (local LLMs) directly inside SwarmUI for prompt generation, enhancement, captions, and vision—no separate chat UI needed.

Installation options follow the [MagicPrompt Installation](https://github.com/HartsyAI/SwarmUI-MagicPromptExtension?tab=readme-ov-file#installation) guide: **automatic** (recommended) or **manual**.

Extensions live in `swarmui/extensions/` (source); the recompiled app lives in `swarmui/bin/` (persisted). Run `make setup` before first use so both directories exist.

## Automatic install (recommended)

1. **Start SwarmUI and Ollama** (e.g. `make up` or `make sui`; or `make llm` for Ollama only).
2. **Pull an Ollama model** (if needed):
   ```bash
   make setup-ollama MODEL=llama3.2:1b
   ```
3. In SwarmUI go to **Server → Extensions**, find **MagicPrompt**, and click **Install**. Let it download and follow the prompts to restart Swarm.
4. Open the **MagicPrompt** tab (or **Enhance Prompt** / **Magic Vision** in the Generate tab), then **Settings** and set:
   - **Chat Backend**: Ollama (or another supported backend)
   - **Base URL**: `http://localhost:11434` (with host networking, same machine)
   - **Chat Model** / **Vision Model**: an Ollama model you’ve pulled

You can then enhance prompts, generate captions from images, use `<mpprompt:…>` tags in prompts for batch LLM processing, and more. See the [MagicPrompt README](https://github.com/HartsyAI/SwarmUI-MagicPromptExtension) for full features and configuration (OpenRouter, OpenAI, Anthropic, etc.).

## Manual install

If you prefer to clone the extension yourself (e.g. the UI install isn’t working):

1. **Clone into the Extensions volume** (so SwarmUI sees it as `src/Extensions/<repo-name>`):
   ```bash
   make setup    # ensure swarmui/extensions exists
   git clone https://github.com/HartsyAI/SwarmUI-MagicPromptExtension.git swarmui/extensions/SwarmUI-MagicPromptExtension
   ```
2. **Recompile SwarmUI.** In Freya: **`make swarmui-rebuild`** (runs SwarmUI’s update script if present, otherwise `dotnet build`).
3. **Restart** so the new build is used: **`make down && make sui`**.

Working sequence after adding or changing an extension: **`make swarmui-rebuild`** then **`make down && make sui`**.

Then configure MagicPrompt as in the automatic install section (Settings → Base URL, model, etc.).

## Ollama URL when running in Docker

With Freya’s `network_mode: host`, SwarmUI and Ollama both use the host network, so from inside the SwarmUI container, Ollama is at **`http://localhost:11434`**. If you run SwarmUI or Ollama in a different setup, set the Base URL in MagicPrompt settings to match (e.g. host IP and port 11434).

## Memory and model choices

**What memory does Ollama use?**  
Ollama uses **GPU VRAM** when an NVIDIA GPU is available (same GPU as ComfyUI/SwarmUI). If VRAM is full or no GPU is present, it uses **system RAM** (CPU). Very large context windows can also spill into system RAM. Use `ollama ps` (on the host or in the container) to see what’s loaded and whether it’s on GPU or CPU.

**Rough VRAM use by model size** (typical quantizations, e.g. Q4):

| Model size | Approx. VRAM | Examples |
|------------|--------------|----------|
| 1B         | ~1–2 GB      | `llama3.2:1b` |
| 3B         | ~2–4 GB      | `llama3.2:3b`, `phi3:mini` |
| 7–8B       | ~4–7 GB      | `llama3.2`, `mistral`, `gemma2:7b` |
| 12B+       | ~8 GB+       | `gemma2:9b`, `llama3.1:8b` (larger variants) |

**Models that work well for prompt generation (MagicPrompt):**  
For enhancing or generating SD prompts you want something **fast and small** so the GPU still has room for SwarmUI/ComfyUI. Good options:

- **Light (1–3B):** `llama3.2:1b`, `llama3.2:3b`, `phi3:mini` — minimal VRAM, quick responses, fine for short prompt enhancement.
- **Balanced (7–8B):** `llama3.2`, `mistral`, `gemma2:7b` — better quality and nuance for more detailed or creative prompts; still reasonable VRAM (~5–7 GB).

If you run **SwarmUI + Ollama on one GPU**, prefer 1B or 3B so enough VRAM stays free for image/video models. If you run Ollama alone (`make llm`) or on a separate machine, 7B is a good default for prompt quality.

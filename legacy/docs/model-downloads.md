# Downloading Models from Hugging Face and Civitai

**ComfyUI** has no built-in model downloader. Use the provided script for starter models and to add more: `make download-starter-models` or `make download-model TYPE=<type> URL=<url> [FILE=<filename>]`. See [MODELS.md](MODELS.md) for types and directory layout.

**SwarmUI** has a built-in model download utility — use it from the SwarmUI UI for SwarmUI models.

This page describes how to obtain models from Hugging Face and Civitai (URLs, tokens, API keys) so you can add more ComfyUI models via the script or manual placement.

**Requirements:** An account and API key (or token) are required for both Hugging Face and Civitai when using their APIs or authenticated downloads.

---

## Hugging Face

- **Site:** [huggingface.co](https://huggingface.co)
- **Models:** [huggingface.co/models](https://huggingface.co/models) — filter by pipeline (e.g. text-to-image) or library (e.g. diffusers, LoRA).

### Account and token

1. Create an account at [huggingface.co/join](https://huggingface.co/join).
2. Create a token: [huggingface.co/settings/tokens](https://huggingface.co/settings/tokens).
   - Choose “Read” for normal downloads.
   - For gated models, accept the model’s license on its page first.
3. Use the token where the tools expect it (e.g. `HF_TOKEN` or `HUGGING_FACE_HUB_TOKEN`), or log in via `huggingface-cli login`.

### Downloading

- **In-browser:** Open the model repo, open the file, use “Download”.
- **CLI:** Install and use the Hub client, then download by repo id and filename; use your token for private/gated repos:
  ```bash
  pip install huggingface_hub
  huggingface-cli login   # paste your token when prompted
  huggingface-cli download repo_id --include "filename.safetensors"
  ```
- **Direct URL:** For public files, you can use a “resolve” URL in scripts or browsers:
  `https://huggingface.co/<org>/<repo>/resolve/main/<path_to_file>`
  For gated/private repos, use the API or `huggingface-cli` with your token.

---

## Civitai

- **Site:** [civitai.com](https://civitai.com)
- **Models:** Browse by type (Checkpoint, LoRA, VAE, etc.) and use search/filters.

### Account and API key

1. Create an account at [civitai.com](https://civitai.com).
2. Create an API key: **Profile → Settings → API Keys** (or [civitai.com/user/account](https://civitai.com/user/account)), then “Create API Key”.
3. Use this key in the `Authorization: Bearer <key>` header (or whatever the client expects) when calling Civitai’s API.

### Downloading

- **In-browser:** Open the model page, choose the version/file, click “Download”.
- **API:** Civitai provides download endpoints such as:
  - `GET https://civitai.com/api/v1/model-versions/{versionId}/download`
  - Or model-version “downloadUrl” from the API.
  Use your API key in the request (e.g. `Authorization: Bearer YOUR_API_KEY`). See [Civitai API docs](https://github.com/civitai/civitai/wiki/API) for current routes and parameters.

Use the above with the ComfyUI download script (`make download-model TYPE=… URL=…`) or when placing files manually. For SwarmUI, use its built-in download utility when possible.

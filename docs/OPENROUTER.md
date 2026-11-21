# OpenRouter

Freya uses:

- `POST /api/v1/chat/completions` — plan / enhance / assist  
- `GET /api/v1/images/models` + `POST /api/v1/images` — cloud generation  

OpenRouter does **not** accept arbitrary Civitai LoRA files. For LoRAs, configure a diffusion worker and install via Civitai key + Freya UI or `FreyaModelAsset`.

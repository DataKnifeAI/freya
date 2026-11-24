# OpenRouter

Freya uses:

- `POST /api/v1/chat/completions` — Freya **intent router** (light model), plan / enhance / assist / converse  
- `GET /api/v1/images/models` + `POST /api/v1/images` — cloud still-image generation  
- `GET /api/v1/videos/models` (proxied in `GET /v1/models`) — video catalog for the UI picker  
- `POST /api/v1/videos` + poll + content download — Freya **video** / **gif** generate (async; gif may be ffmpeg-converted from mp4)

### Freya request router (not only `openrouter/auto`)

Before chat/run/generate, Freya classifies modality with a configurable light model (`FREYA_ROUTER_MODEL`, Settings **Router**, default **Auto** → `meta-llama/llama-3.1-8b-instruct`, dolphin3-aligned). Prompt: `packs/instructions/route.txt`.

API shape (also embedded in `POST /v1/chat` as `route`):

```json
{
  "modality": "chat|picture|gif|video",
  "reason": "…",
  "agentModel": null,
  "imageModel": null,
  "source": "router|heuristic"
}
```

Pinned Agent/Image/Video models override suggestions. **`gif` / `video` execute** when a backend is available:

| Modality | Backend |
|----------|---------|
| `video` | OpenRouter T2V (default Auto → `bytedance/seedance-2.5-flash`) |
| `gif` | Local Diffusers multi-frame GIF on the GPU worker when present; else OpenRouter short video (+ optional ffmpeg → gif) |
| `picture` | Local Diffusers worker (LoRAs) or OpenRouter stills |

OpenRouter does **not** accept arbitrary Civitai LoRA files. For LoRAs, configure a diffusion worker and install via Civitai key + Freya UI or `FreyaModelAsset`.

### Privacy prefs (real OpenRouter `provider` knobs)

Persisted via `PUT /v1/settings` → secrets file. Injected on chat / image / video requests:

| Freya setting | OpenRouter field | Notes |
|---------------|------------------|-------|
| Zero Data Retention | `provider.zdr: true` | Per-request enforce only; cannot disable account ZDR |
| Data collection deny | `provider.data_collection: "deny"` | Skip providers that store/train on prompts |
| Chat providers (checkboxes) | *(Freya-side only)* | Vendor prefixes on `/v1/models` chat; not sent as `provider.only`/`ignore` |

When ZDR is on, Freya also filters catalogs:

| Catalog | Request |
|---------|---------|
| Chat / agent | `GET /models?zdr=true` |
| Image | `GET /models?output_modalities=image&zdr=true` |
| Video | `GET /models?output_modalities=video&zdr=true` |

Auto image/video defaults switch to Google models that usually pass ZDR/safety retention. If a generate still hits a data-policy 404, Freya retries once with the ZDR fallback model and surfaces an actionable chat/UI error (Freya Settings toggle + OpenRouter privacy link). Per-request `provider.zdr` can only **tighten** routing — it cannot disable account-wide or guardrail ZDR.

### Chat catalog filters (Freya-side)

Agent/chat picker (`list_chat_models`) accepts **any vendor** that passes a denylist — not a major-provider allowlist or newest-N cap. Dropped: ids with routing suffixes (`:batch`, `:nitro`, `:free`, …), embedding/whisper/tts/moderation/upscale names, image-gen oriented ids, and models without text/`chat/completions`. ZDR catalog query and alignment prefs apply after that.

**Chat provider checkboxes** (Settings → Chat providers): vendors are OpenRouter id prefixes before `/`. Prefs shape:

```json
"chatProviders": { "excluded": ["openai"], "legacyOnly": null }
```

- Unchecked → slug in `excluded` (hidden from chat picker).
- Empty `excluded` → all denylist-passing vendors shown.
- **New catalog vendors default checked** (opt-out): they are not in `excluded`.
- Legacy free-text `providerOnly` / `providerIgnore` migrate on read: ignore → `excluded`; only → `legacyOnly` (include-only until the user toggles a checkbox).
- `/v1/models` also returns `chatProviders.available` for the Settings list.

### Alignment prefs (Freya-side — not OpenRouter flags)

| Pref | Effect |
|------|--------|
| `modelAlignment`: safety / uncensored / balanced | Filters `/v1/models` chat list via `top_provider.is_moderated` + name heuristics (`balanced` = no extra alignment cut) |
| `allowNsfwLocal` | When false, hide NSFW-looking local paths and Civitai cards |
| `instructionTone` | Appends tone guidance to pack system instructions |
| `instructions[]` / `instructionOverrides` | View/edit system prompts (converse, route, chat, plan, suggest, session, prompt-prose, prompt-sd). Overrides persist in `FREYA_SECRETS_FILE`; empty string resets to `packs/instructions/*.txt` |

Account policies: [Terms](https://openrouter.ai/terms) · [Privacy](https://openrouter.ai/privacy) · [Data collection](https://openrouter.ai/docs/guides/privacy/data-collection).

### Metrics (credits / usage)

Freya proxies OpenRouter account/key stats at **`GET /v1/metrics/openrouter`** (Studio Metrics tab).

| OpenRouter route | Works with Freya’s completion key? | What Freya shows |
|------------------|------------------------------------|------------------|
| `GET /api/v1/key` | Yes | Per-key spend: `usage` (all-time), `usage_daily` / `usage_weekly` / `usage_monthly`, optional `limit` / `limit_remaining` / `limit_reset` |
| `GET /api/v1/credits` | **No** — needs a [Management API key](https://openrouter.ai/docs/guides/overview/auth/management-api-keys) | Account `total_credits` − `total_usage` (= remaining). Freya tries this and surfaces `creditsError` when the key is not a management key |

**Gaps:** no official per-generation cost ledger or windowed “recent spend” for a normal key; management-only routes (`/keys`, activity analytics) are not wired. Account wallet balance only appears if you store a management key as `OPENROUTER_API_KEY` (that key cannot call completions — keep completion + management keys separate; Freya uses one slot).

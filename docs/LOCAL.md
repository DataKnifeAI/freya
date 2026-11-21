# Local development

## Interactive UI mock (no API)

Iterate on Freya UI without OpenRouter, Civitai, or a worker:

```bash
make mock-ui
# or: cd web && npm i && npm run dev:mock
```

Also: open the normal UI with `?mock=1`, or click **Remember mock** in the banner (stores `localStorage.freya.mock=1`).

Mock covers Generate, Enhance, Styles, Civitai search/install flow, Gallery remix, Settings, and agentic **Plan / Auto / YOLO** sessions — in-memory with SVG placeholders. Featured “Use example” is the `freya.png` brand NL prompt (winged warrior / canyon waterfall).

## Full local stack

```bash
cp .env.example .env   # fill OPENROUTER_API_KEY (required); others optional
set -a && source .env && set +a

make run-api
# other terminal
cd web && npm i && npm run dev

# optional GPU / remote stub
make run-worker
# Settings → worker URL http://127.0.0.1:8091
# Settings → paste Civitai API key → Civitai tab → Install to worker
```

In the UI, session automation modes:

| Mode | Behavior |
|------|----------|
| **Plan** | Steps, Civitai query, LoRAs, batch prompts only — no generate/install |
| **Auto** | Runs the plan; pauses on ambiguous / high-impact choices |
| **YOLO** | Full auto — search, install when a worker is set, else generate via OpenRouter |

Worker is optional: cloud generate uses OpenRouter; LoRA install needs `FREYA_DIFFUSION_WORKER_URL` + Civitai.

### Civitai in the UI

- Search/Suggest returns installable types (LoRA, LoCon, DoRA, Checkpoint, TextualInversion, VAE) with thumbnails and type badges — not LoRA-only.
- **Pin** = session-scoped (current generate). **★ Favorite** = persistent library in `localStorage` (`freya.civitaiFavorites`); Civitai tab → Favorites.
- Click a card for details (previews, triggers, base model, related checkpoints). Related assets are Freya-derived from `baseModel` (Civitai has no hard dependency graph).
- API: `GET /v1/civitai/search`, `GET /v1/civitai/models/{id}`, `POST /v1/civitai/install`.

### Video generation (OpenRouter — discovery only)

OpenRouter supports async video via `POST /api/v1/videos` + poll, and catalogs at `GET /api/v1/videos/models` or `GET /api/v1/models?output_modalities=video`. Freya’s `GET /v1/models` now includes a `video` array for discovery; Settings shows a short note. Freya does **not** generate video in the UI yet (image path stays sync `/images`).

### API / worker auth (non-dev)

By default `FREYA_API_TOKEN` / `FREYA_WORKER_TOKEN` are empty and `/v1` routes are open — both binaries log a warning (local/dev). For anything exposed beyond localhost:

```bash
export FREYA_API_TOKEN='long-random-secret'
export FREYA_WORKER_TOKEN='long-random-secret'   # same value on freya-api + freya-worker
# optional: restrict browser origins (comma-separated)
export FREYA_CORS_ORIGINS='https://freya.example.com,http://127.0.0.1:5173'
# optional for Vite UI: VITE_FREYA_API_TOKEN=$FREYA_API_TOKEN
# or paste the same value in UI Settings → Studio API token
```

Send `Authorization: Bearer $FREYA_API_TOKEN` on `/v1` calls. The UI forwards a token from Settings / `VITE_FREYA_API_TOKEN` / `localStorage.freya.apiToken` (nginx does not inject one). Gallery image GETs stay open so `<img src>` works; list/search/settings require the bearer when the token is set.

In-cluster: set `FreyaStudio.spec.apiTokenSecretRef` and `FreyaDiffusionWorker.spec.authSecretRef`. Container UI proxies via `FREYA_API_UPSTREAM` (operator sets this; see `web/nginx.conf`).

`make ci` runs fmt, clippy, and tests.

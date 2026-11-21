# Local development

## Kind cluster UI (`freya.dataknife.net`)

Local kind maps host ports **80/443** to ingress-nginx (`kind-freya.yaml`). The FreyaStudio Ingress host is `freya.dataknife.net`.

**404 page not found** (plain text, `content-type: text/plain`) is **Traefik on prd-apps**, not Freya. Public DNS still CNAMEs `freya.dataknife.net` → `prd-apps-wk.dataknife.net` (192.168.14.x). Override on this machine:

```bash
make kind-hosts   # or ./scripts/kind-hosts.sh (rewrites /etc/hosts + resolvectl flush-caches)
```

This host’s `nsswitch` prefers `resolve` over `files`; systemd-resolved still honors `/etc/hosts` after a cache flush. Without the override you hit Traefik on prd-apps.

### Brave (and other browsers) still 404 after hosts

**Cause:** Brave **Secure DNS** (DNS-over-HTTPS) ignores `/etc/hosts` and resolves via Cloudflare/Google → prd-apps Traefik → `404 page not found`.

**Fix (this machine):**

1. Brave → **Settings** → **Privacy and security** → **Use secure DNS** → **turn Off**  
   (or use “OS default” / a resolver that does **not** bypass system hosts).
2. Hard-reload `https://freya.dataknife.net/` (or open a new private window after the change).

`curl` and browsers that use system DNS already work once hosts is set.

**Other device on the LAN:** kind publishes **80/443** on this host’s LAN IP (`192.168.3.125`). On that device, either:

- Add hosts: `192.168.3.125 freya.dataknife.net` (and disable Secure DNS if the browser uses DoH), or  
- Temporarily: `curl -H 'Host: freya.dataknife.net' http://192.168.3.125/`

NodePort (`31026`/`30556`) is only inside the kind node network; use host **80/443**, not NodePort from outside.

Working URLs after the hosts override (and Secure DNS off in Brave):

| URL | Expected |
|-----|----------|
| https://freya.dataknife.net/ | Freya UI (200) |
| http://freya.dataknife.net/ | Freya UI (200) |
| https://freya.dataknife.net/v1/health | API health JSON |
| https://vllm.dataknife.net/health | Local diffusion worker (DNS already → this host) |

Without hosts: `curl -H 'Host: freya.dataknife.net' http://127.0.0.1/` also serves the UI. Vite mock/dev remains at http://127.0.0.1:5173/.

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

### Local worker on game drive vs cluster vLLM

| Path | URL / endpoint | Models |
|------|----------------|--------|
| **This machine (local)** | `FREYA_DIFFUSION_WORKER_URL=http://127.0.0.1:8091` (binds `0.0.0.0:8091`) | `FREYA_MODELS_DIR=/mnt/game2/freya/models` (repo `data/models` → symlink) |
| **Cluster Studio** | `FreyaDiffusionWorker` `externalEndpoint` (e.g. `https://vllm.dataknife.net`) | Remote GPU / vLLM — do not overwrite that CR when only setting up the local worker |

Worker health/stats:

| Endpoint | Notes |
|----------|--------|
| Worker `GET /health` / `GET /v1/stats` | `{ ok, modelsDir, loras, checkpoints, uptimeSec, vram, host }` — VRAM from `nvidia-smi` when present (null in GPU-less containers) |
| API `GET /v1/worker/stats` | Proxies worker stats; fills host `nvidia-smi` when worker VRAM is null; includes docker/systemd runtime |
| API `POST /v1/worker/start\|stop` | Local docker (`FREYA_WORKER_CONTAINER`, default `freya-local-worker`) or systemd unit |
| API `POST /v1/worker/enable\|disable` | Persist flag in secrets — disabled skips local gens; OpenRouter still works |

Settings shows a compact VRAM bar + Start / Stop / Enable / Disable.

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

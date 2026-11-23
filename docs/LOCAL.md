# Local development

## Single-system Docker Compose (no k8s)

One machine, no operator/CRDs: API + nginx UI + local worker (+ optional MCP).

```bash
cp deploy/compose/.env.example deploy/compose/.env
# set OPENROUTER_API_KEY + FREYA_ADMIN_PASSWORD
make compose-up          # UI :8080  (worker stays on the compose network)
# make compose-up-gpu    # NVIDIA overlay
# make compose-up-mcp    # + MCP on 127.0.0.1:8090 (needs FREYA_API_TOKEN)
```

See **[COMPOSE.md](COMPOSE.md)** for volumes, auth, and Compose vs k8s+external-worker. Legacy Comfy/Swarm Compose remains under `legacy/` only.

## Default story (shared k8s + local GPU worker)

| In cluster | Out of cluster |
|------------|----------------|
| Operator, FreyaStudio (API / UI / MCP), Ingress | GPU worker on this host (`make run-worker` / Docker `freya-local-worker`) |

Apply the **external** worker sample (`config/samples/freya_v1alpha1_freyadiffusionworker_external.yaml`) and point `externalEndpoint` (or the Worker tab Host/Port / Advanced URL) at the GPU process — LAN IP, FQDN, `localhost` when the API can see the host, or an ExternalName/Endpoints bridge. Freya does **not** need a dedicated cluster or an in-cluster GPU. **kind** below is a **dev** convenience; production-shaped is the same CRs on shared k8s + that worker URL. See [ARCHITECTURE.md](ARCHITECTURE.md).

## Kind cluster UI (`freya.dataknife.net`) — local **dev** path

Local kind maps host ports **80/443** to ingress-nginx (`kind-freya.yaml`). The FreyaStudio Ingress host is `freya.dataknife.net`.

### TLS (`*.dataknife.net` wildcard)

Same pattern as prd Traefik / Harbor / slashbay: Let's Encrypt secret `wildcard-dataknife-net-tls` (SANs `*.dataknife.net`, `dataknife.net`).

```bash
make kind-tls   # copies secret from ~/.kube/prd-apps-rke2.yaml → kube-system + freya;
                # sets ingress-nginx --default-ssl-certificate; fixes freya-vllm tls
```

FreyaStudio sample sets `ingress.tlsSecretName: wildcard-dataknife-net-tls`. Without that field, HTTPS still works once nginx’s default cert is set (slashbay-style). Do **not** use a self-signed Ingress secret.

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
| https://freya.dataknife.net/mcp | Freya MCP Streamable HTTP (POST JSON-RPC; GET may 405 — not a browser page) |
| https://vllm.dataknife.net/health | Local diffusion worker (DNS already → this host) |

### Freya MCP (Cursor / agents)

Operator deploys MCP with the Studio (`mcpEnabled: true` by default). Public endpoint:

```text
https://freya.dataknife.net/mcp
```

Cursor MCP config (Streamable HTTP):

```json
{
  "mcpServers": {
    "freya": {
      "url": "https://freya.dataknife.net/mcp"
    }
  }
}
```

Local without cluster: `make run-api` then `make run-mcp` → `http://127.0.0.1:8090/mcp` (set `FREYA_API_TOKEN` if the API requires it). Kind rebuild: `make kind-deploy-mcp`. Tools: `health`, `settings`, `generate_image`, `gallery_list`, `gallery_get`, `civitai_search`, `worker_stats`, `models`.

Without hosts: `curl -H 'Host: freya.dataknife.net' http://127.0.0.1/` also serves the UI. Vite mock/dev remains at http://127.0.0.1:5173/.

### Studio admin login (game-operator style)

With `generateAdminSecrets: true` (default), the operator creates Secret `freya-admin`
(`username` + `admin-password`). Open the UI → **Sign in**, or:

```bash
kubectl -n freya get secret freya-admin -o jsonpath='{.data.admin-password}' | base64 -d; echo
```

Machine MCP keeps using Secret `freya-api-token`. See [AUTH.md](AUTH.md).

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

# GPU worker on this host (recommended; out of cluster)
make run-worker
# Worker tab → Host/Port → http://127.0.0.1:8091 (or LAN IP / FQDN from in-cluster Studio)
# Settings → paste Civitai API key → Civitai tab → Install to worker
```

In the UI, session automation modes:

| Mode | Behavior |
|------|----------|
| **Plan** | Steps, Civitai query, LoRAs, batch prompts only — no generate/install |
| **Auto** | Runs the plan; pauses on ambiguous / high-impact choices |
| **YOLO** | Full auto — search, install when a worker is set, else generate via OpenRouter |

Worker is optional: cloud generate uses OpenRouter; LoRA install needs `FREYA_DIFFUSION_WORKER_URL` + Civitai.

### Local GPU worker (recommended) vs optional in-cluster GPU

| Path | URL / endpoint | Models |
|------|----------------|--------|
| **This machine (recommended)** | `make run-worker` / `freya-local-worker` → `http://127.0.0.1:8091` (binds `0.0.0.0:8091`); from in-cluster Studio use LAN IP, FQDN (`https://vllm.dataknife.net` on this lab), or ExternalName | `FREYA_MODELS_DIR=/mnt/game2/freya/models` (repo `data/models` → symlink); SD/SDXL/Pony/Flux live here |
| **Cluster Studio → external worker** | `FreyaDiffusionWorker` `placement: external` + `externalEndpoint` (sample YAML) — same URL the Worker tab saves | Operator sets `FREYA_DIFFUSION_WORKER_URL` from the CR; no in-cluster GPU required |
| **Optional in-cluster GPU** | `placement: incluster` sample — only when the cluster has `nvidia.com/gpu` | Operator Deployment + models PVC; **not** the default story |

#### Models on disk (`FREYA_MODELS_DIR`)

```
models/
  loras/           *.safetensors + optional *.safetensors.meta.json  (baseModel, versionId, …)
  checkpoints/     *.safetensors + optional *.meta.json
                   (recommended Pony: checkpoints/pony-diffusion-v6-xl.safetensors)
  embeddings/      (TextualInversion)
  vae/
  active-base.json   { "model": "checkpoints/pony-diffusion-v6-xl.safetensors", "baseModel": "Pony" }
```

Install from the Civitai UI writes into these folders. Sidecar `.meta.json` stores Civitai `baseModel` so generate can pick a compatible checkpoint. Only **one** active base is selected at a time (VRAM); swap via Settings or `PUT /v1/worker/base-model`.

Worker health/stats:

| Endpoint | Notes |
|----------|--------|
| Worker `GET /health` / `GET /v1/stats` | `{ ok, modelsDir, loras, checkpoints, uptimeSec, vram, host, modelLoaded, layout }` — VRAM from `nvidia-smi` when present (null in GPU-less containers) |
| Worker `GET /v1/checkpoints` | List installed checkpoints (+ `baseModel` from sidecar) |
| Worker `GET\|PUT /v1/base-model` | Active base selection (persisted as `active-base.json`) |
| API `GET /v1/worker/stats` | Proxies worker stats; fills host `nvidia-smi` when worker VRAM is null; includes docker/systemd runtime |
| API `GET /v1/worker/checkpoints` | Proxies checkpoint list |
| API `GET\|PUT /v1/worker/base-model` | Proxies active base |
| API `GET /v1/base-models` | Static family catalog (SD 1.5 / SDXL / Pony / Flux hints) — no Civitai traffic |
| API `POST /v1/worker/start\|stop` | Local docker (`FREYA_WORKER_CONTAINER`, default `freya-local-worker`) or systemd unit |
| API `POST /v1/worker/enable\|disable` | Persist flag in secrets — disabled skips local gens; OpenRouter still works |

**Worker tab** (not Settings): host/port (defaults `localhost`/`8091`), Setup copy for LAN / FQDN / k8s DNS bridge, Generate API token, VRAM bar, **active base** picker, Start / Stop / Enable / Disable. Settings keeps OpenRouter/Civitai keys only — the diffusion URL lives on Worker.

**GPU engine (this host):** `./scripts/run-local-worker.sh` (or `make run-local-worker`) builds `Dockerfile.worker` and runs Docker `freya-local-worker` with `--gpus all`. Inside the container, freya-worker proxies generate to a **Diffusers** sidecar (`FREYA_ENGINE_URL=http://127.0.0.1:8092`). Checkpoints + Civitai LoRAs under `FREYA_MODELS_DIR` load **on demand** into VRAM and unload after each generate unless `FREYA_ENGINE_KEEP_LOADED=1`. Health `modelLoaded.loadedInVram` reflects the engine; `nvidia-smi` VRAM appears when the container has GPU access. Compose: `make compose-up-gpu`. Without `FREYA_ENGINE_URL`, generate still returns a stub PNG for wiring tests. CRD `engine: vllm-omni` remains the label for swapping to vLLM-Omni on the same HTTP surface.

### Civitai in the UI

- Search/Suggest returns installable types (LoRA, LoCon, DoRA, Checkpoint, TextualInversion, VAE) with thumbnails and type badges — not LoRA-only.
- **Pin** = session-scoped (current generate). **★ Favorite** = persistent library in `localStorage` (`freya.civitaiFavorites`); Civitai tab → Favorites.
- Click a card for details (previews, triggers, base model, related checkpoints). Related assets are Freya-derived from `baseModel` (Civitai has no hard dependency graph).
- Install / pin now forward `baseModel` (+ `versionId` / `modelType`) to the worker; generate sends `path` + `baseModel` so the API can select a matching active base.
- API: `GET /v1/civitai/search`, `GET /v1/civitai/models/{id}`, `POST /v1/civitai/install`.

### Intent router (picture / gif / video / chat)

`POST /v1/chat`, `/v1/run`, and `/v1/generate` run Freya’s light router first (`packs/instructions/route.txt`). Defaults: Agent / Image / Video / Router pickers **Auto**. Router Auto → `meta-llama/llama-3.1-8b-instruct`; Image Auto → Flux NL; Video Auto → `bytedance/seedance-2.5-flash`. GIF/video asks **execute** (OpenRouter T2V and/or local Diffusers GIF) — they no longer hard-reject. See [CONCEPTS.md](CONCEPTS.md) and [OPENROUTER.md](OPENROUTER.md).

### Video / GIF

| Path | Behavior |
|------|----------|
| **OpenRouter video** | `POST /videos` → poll → gallery `video/mp4` (UI `<video>`); gif modality may convert with host `ffmpeg` |
| **Local GIF** | GPU worker `POST /v1/videos/generations` `format=gif` — multi-frame Diffusers animation under `FREYA_MODELS_DIR` |
| **VRAM** | Stills/GIF: on-demand load into VRAM, unload after generate unless `FREYA_ENGINE_KEEP_LOADED=1`. OpenRouter video uses cloud GPUs (no local VRAM). |

UI: gallery + chat show video players / animated GIFs; chrome **Video** picker lists catalog models (avatar/lip-sync ids filtered from Auto).

### API / worker auth (non-dev)

By default `FREYA_API_TOKEN` / `FREYA_WORKER_TOKEN` are empty and `/v1` routes are open — both binaries log a warning (local/dev). For anything exposed beyond localhost:

```bash
export FREYA_API_TOKEN='long-random-secret'
export FREYA_WORKER_TOKEN='long-random-secret'   # same value on freya-api + freya-worker
# optional: restrict browser origins (comma-separated)
export FREYA_CORS_ORIGINS='https://freya.example.com,http://127.0.0.1:5173'
# optional for Vite UI: VITE_FREYA_API_TOKEN=$FREYA_API_TOKEN
# or Settings → Generate API token (preferred) / Advanced paste
```

**UI:** Worker tab → **Generate API token** calls `POST /v1/auth/token` (rotates in-memory + `FREYA_SECRETS_FILE`). The UI **re-applies** the bearer to `localStorage.freya.apiToken` immediately so this tab does not 401 after rotate. In-cluster the operator sets `FREYA_SECRETS_FILE=/data/gallery/.freya-runtime.json` on the gallery PVC so rotates survive API pod restarts, and wires `FREYA_API_TOKEN_SECRET_NAME` so the API **patches** Secret `freya-api-token` when RBAC is present. If Secret and runtime file diverge (classic 401 for MCP/curl), run `./scripts/sync-api-token-from-pvc.sh` then `kubectl -n freya rollout restart deploy/freya-mcp`. Other browsers: Worker → Advanced → paste → **Apply token**.

Send `Authorization: Bearer $FREYA_API_TOKEN` on `/v1` calls. The UI forwards a token from Worker/Settings / `VITE_FREYA_API_TOKEN` / `localStorage.freya.apiToken` (nginx does not inject one). Gallery image GETs stay open so `<img src>` works; list/search/settings require the bearer when the token is set.

**Worker tab endpoint:** host + port (defaults `localhost` / `8091` via Use local defaults) composed to `http://host:port` — same forms as `FreyaDiffusionWorker.spec.externalEndpoint` (LAN IP, FQDN, localhost, optional k8s DNS bridge). Full URL override is under Advanced (https).

In-cluster Studio: set `FreyaStudio.spec.apiTokenSecretRef` and `FreyaDiffusionWorker.spec.authSecretRef`. Container UI proxies via `FREYA_API_UPSTREAM` (operator sets this; see `web/nginx.conf`).

`make ci` runs fmt, clippy, and tests.

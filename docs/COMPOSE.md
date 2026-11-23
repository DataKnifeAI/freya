# Single-system Docker Compose

Run Freya on **one machine** without Kubernetes, kind, or the Freya operator. API + UI (+ optional MCP) and a local GPU worker share a Compose network. Host directories hold gallery, models, and secrets.

The old ComfyUI / SwarmUI / Ollama stack stays under [`legacy/docker-compose.yml`](../legacy/docker-compose.yml) — do not mix the two.

## Compose vs k8s + external worker

| Path | What runs where | When to use |
|------|-----------------|-------------|
| **Compose (this doc)** | API, nginx UI, worker (+ optional MCP) on one host via `deploy/compose/` | Laptop / single GPU box; no cluster |
| **k8s + external worker** | Operator + FreyaStudio (API/UI/MCP/Ingress) in cluster; worker on the GPU host by URL | Shared cluster + lab GPU; see [LOCAL.md](LOCAL.md) / [ARCHITECTURE.md](ARCHITECTURE.md) |
| **Local cargo + Vite** | `make run-api` + `npm run dev` (+ `make run-worker`) | Day-to-day UI/API development |

Compose does **not** use FreyaStudio / FreyaDiffusionWorker CRDs. Admin login is env-based (`FREYA_ADMIN_USER` / `FREYA_ADMIN_PASSWORD`).

## Quick start

```bash
cp deploy/compose/.env.example deploy/compose/.env
# edit: OPENROUTER_API_KEY, FREYA_ADMIN_PASSWORD (openssl rand -base64 24)
# optional: CIVITAI_API_KEY, FREYA_API_TOKEN, FREYA_WORKER_TOKEN

# Host data dirs (lab defaults). Distroless images run as UID 65532.
mkdir -p /mnt/game2/freya/{gallery,models,secrets}
sudo chown -R 65532:65532 /mnt/game2/freya/gallery /mnt/game2/freya/secrets /mnt/game2/freya/models

make compose-up          # build + start api, web, worker
# make compose-up-gpu    # same + nvidia device reservation
# make compose-up-mcp    # also start freya-mcp (:8090)
```

Equivalent without Make:

```bash
docker compose -f deploy/compose/compose.yaml --env-file deploy/compose/.env up -d --build
```

Stop:

```bash
make compose-down
# or: docker compose -f deploy/compose/compose.yaml --env-file deploy/compose/.env down
```

## Ports

| Service | Host port (default) | Notes |
|---------|---------------------|--------|
| **freya-web** (nginx UI) | **8080** | Proxies `/v1/` → `freya-api:8080` |
| **freya-worker** | (not published) | API uses `http://freya-worker:8091` on the Compose network |
| **freya-mcp** (profile `mcp`) | **127.0.0.1:8090** | Streamable HTTP at `/mcp`. Requires `FREYA_API_TOKEN` |
| freya-api | (internal only) | Reachable as `http://freya-api:8080` on the Compose network |

Open **http://127.0.0.1:8080/** → Sign in with `FREYA_ADMIN_USER` / `FREYA_ADMIN_PASSWORD`. The API already has `FREYA_DIFFUSION_WORKER_URL=http://freya-worker:8091`. The worker port is not published on the host.

## Volumes / host paths

| Env | Default (lab) | Container mount |
|-----|---------------|-----------------|
| `FREYA_GALLERY_DIR` | `/mnt/game2/freya/gallery` | `/data/gallery` |
| `FREYA_MODELS_DIR` | `/mnt/game2/freya/models` | `/data/models` |
| `FREYA_SECRETS_DIR` | `/mnt/game2/freya/secrets` | `/data/secrets` (runtime → `.freya-runtime.json`) |

Portable: set those to `./data/gallery`, `./data/models`, `./data/secrets` in `deploy/compose/.env` (paths relative to `deploy/compose/`).

## Auth (no operator)

| Mechanism | Compose |
|-----------|---------|
| Admin UI login | Set `FREYA_ADMIN_PASSWORD` in `.env` (and optional `FREYA_ADMIN_USER`) |
| Machine bearer | `FREYA_API_TOKEN` — required for MCP (`/mcp` returns 401 when empty). UI can also rotate via Worker → Generate API token (persists under secrets dir) |
| Worker bearer | Optional `FREYA_WORKER_TOKEN` — same value on API + worker |

Empty tokens + empty admin password leave `/v1` open (local/dev warning). Prefer at least an admin password for anything beyond loopback.

## GPU

Default `make compose-up` runs the **stub** worker (no NVIDIA devices; generate returns placeholder PNGs; VRAM may be null).

With [nvidia-container-toolkit](https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/install-guide.html):

```bash
make compose-up-gpu
```

That merges [`deploy/compose/compose.gpu.yaml`](../deploy/compose/compose.gpu.yaml), builds [`Dockerfile.worker`](../Dockerfile.worker) (Diffusers + CUDA), and reserves a GPU. Weights load on demand from `FREYA_MODELS_DIR` and unload after generate unless `FREYA_ENGINE_KEEP_LOADED=1`.

Standalone GPU host (Studio elsewhere): `./scripts/run-local-worker.sh` → container `freya-local-worker` on `:8091`.

## MCP (optional)

```bash
make compose-up-mcp
# Cursor / agents: http://127.0.0.1:8090/mcp
# Set FREYA_API_TOKEN in .env. Empty token is rejected. Host check stays on localhost.
```

## Images

Compose builds from repo Dockerfiles:

| Service | Dockerfile | Target |
|---------|------------|--------|
| freya-api | `Dockerfile` | `api` |
| freya-worker | `Dockerfile` (`worker`) or `Dockerfile.worker` with GPU overlay | stub / Diffusers+CUDA |
| freya-mcp | `Dockerfile` | `mcp` |
| freya-web | `Dockerfile.web` | (nginx) |

First build compiles the Rust workspace and is slow; later rebuilds use Docker layer cache.

## Makefile targets

| Target | Action |
|--------|--------|
| `compose-up` | Build + start api, web, worker |
| `compose-up-gpu` | Same + GPU overlay |
| `compose-up-mcp` | Same + MCP profile |
| `compose-down` | Stop and remove containers |
| `compose-logs` | Follow logs |
| `compose-ps` | `docker compose ps` |

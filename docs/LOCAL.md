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

Mock covers Generate, Enhance, Styles, Civitai search/install flow, Gallery remix, and Settings — in-memory with SVG placeholders.

## Full local stack

```bash
cp .env.example .env   # fill keys
export $(grep -v '^#' .env | xargs)

make run-api
# other terminal
cd web && npm i && npm run dev

# optional
make run-worker
# Settings → worker URL http://127.0.0.1:8091
# Settings → paste Civitai API key → Civitai tab → Install to worker
```

### API / worker auth (non-dev)

By default `FREYA_API_TOKEN` / `FREYA_WORKER_TOKEN` are empty and `/v1` routes are open — both binaries log a warning (local/dev). For anything exposed beyond localhost:

```bash
export FREYA_API_TOKEN='long-random-secret'
export FREYA_WORKER_TOKEN='long-random-secret'   # same value on API + worker
# optional: restrict browser origins (comma-separated)
export FREYA_CORS_ORIGINS='https://freya.example.com,http://127.0.0.1:5173'
# optional for Vite UI: VITE_FREYA_API_TOKEN=$FREYA_API_TOKEN
# or paste the same value in UI Settings → Studio API token
```

Send `Authorization: Bearer $FREYA_API_TOKEN` on `/v1` calls. The UI forwards a token from Settings / `VITE_FREYA_API_TOKEN` / `localStorage.freya.apiToken` (nginx does not inject one). Gallery image GETs stay open so `<img src>` works; list/search/settings require the bearer when the token is set.

In-cluster: set `FreyaStudio.spec.apiTokenSecretRef` and `FreyaDiffusionWorker.spec.authSecretRef`.

`make ci` runs fmt, clippy, and tests.

# Freya authentication

Studio (API / UI / MCP) and auth Secrets live **in cluster** for the k8s path. The diffusion worker is usually **out of cluster** on the GPU host and authenticates with `FREYA_WORKER_TOKEN` / `FreyaDiffusionWorker.spec.authSecretRef` over its URL — see [ARCHITECTURE.md](ARCHITECTURE.md).

**Compose (single-system):** no operator or admin Secret CRD. Set `FREYA_ADMIN_USER` / `FREYA_ADMIN_PASSWORD` (and optional `FREYA_API_TOKEN` / `FREYA_WORKER_TOKEN`) in `deploy/compose/.env`. See [COMPOSE.md](COMPOSE.md).

## Default / fallback (game-operator style)

Game operators (Palworld / Dragonwilds / Satisfactory Server Manager) provision an
**admin login** Secret (`admin` + `admin-password`) and protect the UI with HTTP Basic.

Freya mirrors that as the human default:

| Piece | Behavior |
|-------|----------|
| `FreyaStudio.spec.generateAdminSecrets` | Default **true**. Operator ensures `{name}-admin` Secret (`username`, `admin-password`) and, if `apiTokenSecretRef` is unset, `{name}-api-token` (`token`). |
| Status | `adminCredentialsSecretName`, `adminCredentialsGenerated`, `apiTokenSecretName` (never the password). |
| API | `FREYA_ADMIN_USER` / `FREYA_ADMIN_PASSWORD` from the admin Secret. Public `GET /v1/auth/status`, `POST /v1/auth/login`. Protected `/v1/*` accepts **Bearer** (machine token) or **Basic** (admin). |
| UI | Login modal + locked banner when unauthenticated; Send / Suggest / Run disabled with reason. |
| Machine clients | MCP / workers keep using the API bearer Secret — not the admin password. |

Read the admin password (operators only):

```bash
kubectl -n freya get secret freya-admin -o jsonpath='{.data.admin-password}' | base64 -d; echo
```

BYO: set `generateAdminSecrets: false` and `adminCredentialsSecretRef.name`, plus `apiTokenSecretRef`.

## Longer-term: Authentik + Stoat

- **Authentik** (`https://auth.dataknife.net`) is the lab IdP (gitops-tools). Most DockFire “Sign in” apps use it.
- **Stoat** (`stoat.dataknife.net`) is self-hosted chat (stoat-operator). Its **admin panel** already uses Authentik OIDC (`authentikId` / `authentikSecret` / `authentikIssuer` Secret) — not Traefik forward-auth on the chat UI itself.
- **Next Freya SSO step** (not implemented here): Authentik OAuth2/OIDC app for `freya.dataknife.net`, CRD fields `spec.oidc.secretRef` + issuer/client, API routes `/v1/auth/oidc/start|callback`, UI “Sign in with Authentik”. Keep admin password + machine bearer as fallback (same split as Stoat admin SSO vs game-operator basic).

Do not invent a Freya-only IdP flow; follow Stoat-admin’s Authentik OIDC provider pattern when SSO lands.

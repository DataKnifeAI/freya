# Architecture

```
User → Freya UI → Freya API
                     ├─ OpenRouter (chat + images)
                     ├─ Civitai API (search; authenticated downloads)
                     └─ Diffusion worker (in-cluster or remote)
                            └─ /v1/images/generations
                            └─ /v1/assets/install  (LoRAs for UI)
```

## Secrets

| Secret | FreyaStudio / Worker field | Env | Notes |
|--------|----------------------------|-----|-------|
| OpenRouter | `spec.openRouter.secretRef` | `OPENROUTER_API_KEY` | Also editable in UI Settings |
| Civitai | `spec.civitai.secretRef` | `CIVITAI_API_KEY` | UI Settings |
| Hugging Face | `spec.huggingFace.secretRef` | `HF_TOKEN` | optional |
| API bearer | `FreyaStudio.spec.apiTokenSecretRef` | `FREYA_API_TOKEN` | UI sends bearer from Settings (not injected by nginx); required for Ingress |
| Worker bearer | `FreyaDiffusionWorker.spec.authSecretRef` | `FREYA_WORKER_TOKEN` | Injected on worker + API |

Never put API keys in CRD string fields — only Secret references.

## Worker placement

- **incluster** — operator creates Deployment + models PVC + Service; Job downloads for `FreyaModelAsset`.
- **external** — status.endpoint = `spec.externalEndpoint`; asset reconcile POSTs `/v1/assets/install` on the remote host.

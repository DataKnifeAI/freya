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

| Secret | FreyaStudio field | Env on API | UI Settings |
|--------|-------------------|------------|-------------|
| OpenRouter | `spec.openRouter.secretRef` | `OPENROUTER_API_KEY` | OpenRouter API key |
| Civitai | `spec.civitai.secretRef` | `CIVITAI_API_KEY` | Civitai API key |
| Hugging Face | `spec.huggingFace.secretRef` | `HF_TOKEN` | (API/env; optional later in UI) |

Never put API keys in CRD string fields — only Secret references.

## Worker placement

- **incluster** — operator creates Deployment + models PVC + Service; Job downloads for `FreyaModelAsset`.
- **external** — status.endpoint = `spec.externalEndpoint`; asset reconcile POSTs `/v1/assets/install` on the remote host.

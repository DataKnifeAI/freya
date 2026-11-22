# Architecture

```
User → Freya UI → Freya API
                     ├─ OpenRouter (chat + images)
                     ├─ Civitai API (search; authenticated downloads)
                     └─ Diffusion worker (local GPU preferred for SD / LoRAs)
                            └─ /v1/images/generations   (model + lora_request)
                            └─ /v1/assets/install         (LoRAs + checkpoints + meta)
                            └─ /v1/loras | /v1/checkpoints
                            └─ /v1/base-model             (active checkpoint selection)
```

**Where bases live:** `FREYA_MODELS_DIR` (`loras/`, `checkpoints/`, `active-base.json`). Civitai `baseModel` is stored in `*.safetensors.meta.json` and used when the UI/API omits an explicit generate `model`. Remote LLM/vLLM hosts are often **not** SD-capable — use the local RTX worker for multi-base hosting.

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

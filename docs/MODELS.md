# Model Management Guide

This guide explains how to manage Stable Diffusion models for ComfyUI and SwarmUI.

## Directory Structure

### ComfyUI Models

ComfyUI requires models to be placed in specific subdirectories:

```
comfyui/models/
├── checkpoints/      # Main Stable Diffusion models (.ckpt, .safetensors)
├── loras/           # LoRA models (.safetensors, .ckpt)
├── vae/             # VAE models (.safetensors, .ckpt)
├── controlnet/      # ControlNet models (.pth, .safetensors)
├── upscale_models/  # Upscaling models (.pth)
├── embeddings/      # Textual inversions (.pt, .bin)
├── clip/            # CLIP models
├── clip_vision/      # CLIP vision models
├── style_models/     # Style models
└── unet/            # UNet models
```

### SwarmUI Models

SwarmUI models go under the data directory:
```
swarmui/data/Models/   # All model types
```

### Video and image-to-video (SwarmUI)

SwarmUI supports **text-to-video (T2V)** and **image-to-video (I2V)**. No extra Freya config is required—use SwarmUI as usual once the right models are in place.

- **Install recommended video models (Wan 2.1 14B fp8 + LTX Video 2B fp8, good for RTX 4090)**: use SwarmUI’s built-in model download utility, or see [docs/model-downloads.md](model-downloads.md) for Hugging Face / Civitai (account and API key required). Wan → `swarmui/data/Models/diffusion_models/`, LTX → `swarmui/data/Models/Stable-Diffusion/`.
- **Image-to-video**: In the Models sub-tab, pick a normal image model (e.g. SDXL or Flux) as the base, then in the **Image To Video** parameter group select the video model. For an external image, use **Init Image** and set **Init Image Creativity** to 0.
- **Where video models go**: Depends on the model (e.g. `swarmui/data/Models/diffusion_models/` or `Stable-Diffusion` subfolder). Use SwarmUI’s **Server → Paths** or the official docs for exact paths.
- **Beginner’s guide**: [Generate Videos With SwarmUI](https://huggingface.co/blog/MonsterMMORPG/beginners-guide-generate-videos-with-swarmui) (Hugging Face) — step-by-step T2V, text-to-image-to-video, and direct I2V; model pick (e.g. Wan 2.1 14B fp8 for 4090), params, and Init Image Creativity=0. See also [SwarmUI Video Guide](swarmui-video-guide.md) in this repo.
- **Official reference**: [SwarmUI Video Model Support](https://github.com/mcmonkeyprojects/SwarmUI/blob/master/docs/Video%20Model%20Support.md) — install and usage for Hunyuan Video, Wan 2.1/2.2, LTX Video, Kandinsky 5, and others.

## Popular Models

### Base Checkpoints

**Stable Diffusion XL (SDXL) - Most Popular Overall:**
- [stabilityai/stable-diffusion-xl-base-1.0](https://huggingface.co/stabilityai/stable-diffusion-xl-base-1.0)
- File: `sd_xl_base_1.0.safetensors` (6.94 GB)
- **Best for**: High-quality realistic images, commercial content, influencer images
- **Why popular**: Dual text encoders, 1024×1024 native resolution, superior detail, fewer artifacts
- **Recommended for**: Most users starting out

**Stable Diffusion 1.5 - Still Widely Used:**
- [runwayml/stable-diffusion-v1-5](https://huggingface.co/runwayml/stable-diffusion-v1-5)
- File: `v1-5-pruned-emaonly.safetensors` (4.27 GB)
- **Best for**: Artistic styles, anime, conceptual visuals, lower VRAM requirements
- **Why popular**: Extensive LoRA/ControlNet support, well-established ecosystem, smaller file size
- **Recommended for**: Artistic work, users with limited GPU memory, anime/artistic styles

**FLUX.1 - Next Generation:**
- [black-forest-labs/FLUX.1-dev](https://huggingface.co/black-forest-labs/FLUX.1-dev)
- File: `flux1-dev.safetensors` (23.4 GB)
- **Best for**: Sophisticated details, readable text, accurate anatomy, natural language prompts
- **Why popular**: State-of-the-art quality, excellent prompt understanding
- **Note**: Requires significant GPU memory (24GB+ recommended)

### VAE Models

**VAE for SD 1.5:**
- [stabilityai/sd-vae-ft-mse-original](https://huggingface.co/stabilityai/sd-vae-ft-mse-original)
- File: `vae-ft-mse-840000-ema-pruned.safetensors` (334 MB)

**VAE for SDXL:**
- [stabilityai/sdxl-vae](https://huggingface.co/stabilityai/sdxl-vae)
- File: `sdxl_vae.safetensors` (335 MB)

### LoRA Models

Popular LoRA sources:
- [Civitai](https://civitai.com/) - Large collection of LoRA models
- [Hugging Face LoRA](https://huggingface.co/models?pipeline_tag=text-to-image&library=lora)

### ControlNet Models

**SD 1.5 ControlNet:**
- [lllyasviel/control_v11p_sd15_canny](https://huggingface.co/lllyasviel/control_v11p_sd15_canny)
- [lllyasviel/control_v11p_sd15_openpose](https://huggingface.co/lllyasviel/control_v11p_sd15_openpose)

**SDXL ControlNet:**
- [diffusers/controlnet-canny-sdxl-1.0](https://huggingface.co/diffusers/controlnet-canny-sdxl-1.0)

## Downloading Models

### ComfyUI (no built-in downloader)

Use the provided script for starter models and to add more:

- **Starter models (SDXL base + VAE, ~7.3 GB):** `make download-starter-models` or `make quick-setup`
- **Add one model:** `make download-model TYPE=<type> URL=<url> [FILE=<filename>]`  
  Types: `checkpoint`, `lora`, `vae`, `controlnet`, `upscale`, `embedding`

For where to get URLs (Hugging Face, Civitai) and API keys, see **[docs/model-downloads.md](model-downloads.md)**. Both sources require an account and API key/token for API or authenticated downloads. You can also download in the browser and place files under `comfyui/models/<type>/` per [Directory Structure](#directory-structure) above.

### SwarmUI

SwarmUI has a built-in model download utility — use it from the SwarmUI UI (http://localhost:7801) for SwarmUI models. For Hugging Face / Civitai URLs and API keys, see [model-downloads.md](model-downloads.md).

## Model Formats

### SafeTensors (Recommended)
- `.safetensors` - Safer format, faster loading
- Recommended for all new downloads

### PyTorch
- `.ckpt` - Traditional PyTorch checkpoint
- `.pth` - PyTorch model file
- `.pt` - PyTorch tensor/embedding

### Other
- `.bin` - Binary format (embeddings)
- `.onnx` - ONNX format (less common)

## Model Sizes

Typical model sizes:
- **SD 1.5 Checkpoint**: ~4-7 GB
- **SDXL Checkpoint**: ~7-13 GB
- **Flux Checkpoint**: ~24 GB
- **LoRA**: ~10-200 MB
- **VAE**: ~300-400 MB
- **ControlNet**: ~1.5-2 GB
- **Embedding**: ~1-10 MB

## Storage Requirements

Minimum recommended:
- **ComfyUI models**: 50-100 GB
- **SwarmUI models**: 50-100 GB
- **Outputs**: 20-50 GB

For a full collection:
- **ComfyUI**: 200-500 GB
- **SwarmUI**: 200-500 GB

## Model Sources

### Hugging Face
- **URL**: https://huggingface.co/models
- **Search**: Filter by "text-to-image" or "stable-diffusion"
- **Account + token required** for API and gated models. See [model-downloads.md](model-downloads.md).

### Civitai
- **URL**: https://civitai.com/
- **Features**: Ratings, previews, community feedback
- **Account + API key required** for API downloads. See [model-downloads.md](model-downloads.md).

### Stability AI
- **URL**: https://stability.ai/
- **Official**: SD 1.5, SDXL, Flux models

### Other Sources
- **Reddit**: r/StableDiffusion
- **Discord**: Various Stable Diffusion communities
- **GitHub**: Model repositories

## Troubleshooting

### Model Not Showing Up
1. Check file is in correct directory
2. Verify file format (.safetensors, .ckpt, etc.)
3. Restart the service: `make restart`
4. Check file permissions

### Out of Memory
- Use smaller models (SD 1.5 instead of SDXL)
- Reduce image resolution
- Close other GPU applications

### Slow Loading
- Use .safetensors format (faster than .ckpt)
- Ensure models are on fast storage (SSD)
- Check GPU memory availability

### Corrupted Downloads
- Re-download the model
- Verify file size matches source
- Check disk space availability

## Best Practices

1. **Use SafeTensors**: Prefer .safetensors over .ckpt for safety and speed
2. **Organize**: Keep models organized in proper directories
3. **Backup**: Backup important models
4. **Version Control**: Note model versions/names
5. **Storage**: Use fast storage (SSD) for models
6. **Naming**: Use descriptive filenames

## Model Compatibility

### ComfyUI
- Supports SD 1.5, SDXL, Flux, and most variants
- Requires models in specific directory structure
- Auto-detects models on startup

### SwarmUI
- Supports SD 1.5, SDXL, and most variants
- Can use same models as ComfyUI
- More flexible model organization

## Sharing Models Between UIs

Both ComfyUI and SwarmUI can share models:

```bash
# Create symlink to share checkpoints
ln -s ../../../comfyui/models/checkpoints swarmui/data/Models/checkpoints

# Or copy models (uses more space)
cp -r comfyui/models/checkpoints/* swarmui/data/Models/
```

**Note**: SwarmUI may need models in different locations. Check SwarmUI documentation for exact paths.

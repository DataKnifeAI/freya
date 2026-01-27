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

### Using the Download Script

```bash
# Make script executable
chmod +x scripts/download-model.sh

# Download a checkpoint
./scripts/download-model.sh checkpoint \
  https://huggingface.co/runwayml/stable-diffusion-v1-5/resolve/main/v1-5-pruned-emaonly.safetensors

# Download a LoRA with custom filename
./scripts/download-model.sh lora \
  https://civitai.com/api/download/models/12345 \
  my-custom-lora.safetensors

# Download a VAE
./scripts/download-model.sh vae \
  https://huggingface.co/stabilityai/sd-vae-ft-mse-original/resolve/main/vae-ft-mse-840000-ema-pruned.safetensors
```

### Manual Download

1. **From Hugging Face:**
   - Navigate to the model page
   - Click on the file you want
   - Click "Download" button
   - Place in appropriate directory

2. **From Civitai:**
   - Browse models at https://civitai.com/
   - Click "Download" on desired model
   - Place in appropriate directory

3. **Using wget/curl:**
   ```bash
   # For Hugging Face (use "resolve/main" in URL)
   wget -O comfyui/models/checkpoints/model.safetensors \
     https://huggingface.co/user/model/resolve/main/file.safetensors
   
   # For direct links
   curl -L -o comfyui/models/checkpoints/model.safetensors \
     https://example.com/model.safetensors
   ```

### Using Git LFS (for Hugging Face)

Some models use Git LFS. Install git-lfs first:

```bash
# Install git-lfs
sudo apt-get install git-lfs  # Ubuntu/Debian
brew install git-lfs          # macOS

# Clone repository
git lfs install
git clone https://huggingface.co/runwayml/stable-diffusion-v1-5
cp stable-diffusion-v1-5/v1-5-pruned-emaonly.safetensors comfyui/models/checkpoints/
```

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

## Quick Start: Download Essential Models

```bash
# Run setup script first
chmod +x scripts/setup.sh
./scripts/setup.sh

# Download SD 1.5 (essential)
./scripts/download-model.sh checkpoint \
  https://huggingface.co/runwayml/stable-diffusion-v1-5/resolve/main/v1-5-pruned-emaonly.safetensors

# Download VAE for SD 1.5
./scripts/download-model.sh vae \
  https://huggingface.co/stabilityai/sd-vae-ft-mse-original/resolve/main/vae-ft-mse-840000-ema-pruned.safetensors

# Download a popular ControlNet
./scripts/download-model.sh controlnet \
  https://huggingface.co/lllyasviel/control_v11p_sd15_canny/resolve/main/diffusion_pytorch_model.safetensors \
  control_v11p_sd15_canny.safetensors
```

## Model Sources

### Hugging Face
- **URL**: https://huggingface.co/models
- **Search**: Filter by "text-to-image" or "stable-diffusion"
- **Direct Download**: Use "resolve/main" in URL path

### Civitai
- **URL**: https://civitai.com/
- **Features**: Ratings, previews, community feedback
- **API**: Use `/api/download/models/{id}` for direct download

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

# Troubleshooting Guide

## CUDA Base Image Support

Freya’s Dockerfiles use **supported** NVIDIA CUDA container tags. Deprecated tags (e.g. `12.1.0-cudnn8-runtime-ubuntu22.04`) are [unsupported](https://gitlab.com/nvidia/container-images/cuda/-/blob/master/doc/unsupported-tags.md) and scheduled for removal; they also show “THIS IMAGE IS DEPRECATED” at runtime.

- **Policy**: [CUDA Container Support Policy](https://gitlab.com/nvidia/container-images/cuda/blob/master/doc/support-policy.md)
- **Supported tags**: [supported-tags.md](https://gitlab.com/nvidia/container-images/cuda/-/blob/master/doc/supported-tags.md)
- **Current base**: `nvidia/cuda:13.1.1-cudnn-runtime-ubuntu24.04` (Ubuntu 24.04, latest supported CUDA 13.1.1 + cuDNN)

When updating the base image, pick a tag from the [supported list](https://gitlab.com/nvidia/container-images/cuda/-/blob/master/doc/supported-tags.md) for your distro (e.g. ubuntu24.04) and variant (e.g. cudnn-runtime).

## Build Issues

### ComfyUI build fails

- Ensure you have sufficient disk space (build requires ~10GB)
- Check internet connection (clones from GitHub)
- Verify CUDA base image compatibility

### SwarmUI build fails

- Ensure .NET 8 SDK installs correctly
- Check for sufficient memory during build
- Verify Git access to SwarmUI repository
- Verify Python 3.12 and pip installation in Dockerfile

## GPU Not Detected

### Symptoms

- `nvidia-smi` works on host but not in containers
- Error: `could not select device driver "" with capabilities: [[gpu]]`
- `docker info` shows no NVIDIA runtime

### Solution - Install NVIDIA Container Toolkit

```bash
# Ubuntu/Debian
distribution=$(. /etc/os-release;echo $ID$VERSION_ID)
curl -s -L https://nvidia.github.io/nvidia-docker/gpgkey | sudo apt-key add -
curl -s -L https://nvidia.github.io/nvidia-docker/$distribution/nvidia-docker.list | sudo tee /etc/apt/sources.list.d/nvidia-docker.list
sudo apt-get update && sudo apt-get install -y nvidia-container-toolkit
sudo systemctl restart docker

# Verify installation
docker run --rm --gpus all nvidia/cuda:13.1.1-base-ubuntu24.04 nvidia-smi
```

### Run diagnostic script

```bash
chmod +x scripts/linux/check-gpu.sh
./scripts/linux/check-gpu.sh
```

## Port Conflicts

If ports 8188 or 7801 are already in use, modify `docker-compose.yml`:

```yaml
ports:
  - "8189:8188"  # Change host port
```

## Out of Memory

- Reduce image resolution in the UI
- Use smaller models
- Run only one service at a time (or run only the service you need — see [Run only the services you need](#run-only-the-services-you-need))
- Increase GPU memory or use multiple GPUs
- **SwarmUI:** Use the **RAM cleanup** option under System/Server in SwarmUI to free cached RAM between runs
- **Run one stack:** `make sui` (video + MagicPrompt; SwarmUI + Ollama), `make cui` (image), or `make llm` (Ollama only) — each stops other compose services first

## System Freeze with Wan 2.1 I2V on RTX 4090 (24GB)

**Symptom:** The whole system locks up (not just the app) when running Image-to-Video (I2V) with Wan 2.1 14B fp8 on an RTX 4090.

**Cause:** The Wan 2.1 I2V 14B model can use far more than 24GB during inference (reports of ~41–54GB PyTorch-allocated memory). When VRAM is exhausted, the workload spills to system RAM; if that is exhausted too, the system can freeze before the OOM killer acts.

**Workarounds:**

1. **Enable block offloading** (if your SwarmUI/Wan backend supports it): Use `--offload_blocks` with `--offload_blocks_num=1` (or a small number) so part of the model runs on CPU. This reduces GPU usage (e.g. ~11–20GB on 24GB cards) at the cost of slower inference.
2. **Lower output resolution** (e.g. 480p instead of 720p) and/or shorter clip length to reduce peak VRAM.
3. **Close other GPU apps** (browsers, other UIs, ComfyUI) so the 4090 has maximum free VRAM.
4. **Use a lighter video model** for I2V on 24GB (e.g. LTX Video 2B fp8) when Wan 2.1 14B is too heavy.
5. **Free RAM in SwarmUI:** Use the **RAM cleanup** tool under **System** (or Server) options in SwarmUI to release cached memory before or between heavy video runs.
6. **Run only what you need:** When generating video, run SwarmUI + Ollama only so the GPU and RAM aren’t shared. Use `make sui` (SwarmUI + Ollama; stops other services first). For image-only: `make cui`. See [Run only the services you need](#run-only-the-services-you-need) below.

See [Wan 2.1 issues](https://github.com/Wan-Video/Wan2.1/issues) and [SwarmUI Video Model Support](https://github.com/mcmonkeyprojects/SwarmUI/blob/master/docs/Video%20Model%20Support.md) for backend-specific options.

## Run only the services you need

To save GPU VRAM and system RAM, run only the stack you need. Each target stops other compose services, then starts the chosen one(s):

- **All:** `make up`
- **Video + MagicPrompt (SwarmUI + Ollama):** `make sui`
- **Image (ComfyUI only):** `make cui`
- **Ollama only** (for SwarmUI MagicPrompt or API): `make llm`

To stop everything: `make down`.

## First Launch Takes Time

- ComfyUI: Downloads models on first use
- SwarmUI: Initializes database and downloads dependencies
- Both: First generation may be slower as models load into GPU memory

## Migrating from Old SwarmUI Layout

If you previously used separate `swarmui/models` and `swarmui/input` directories, move them under `swarmui/data`:

```bash
# Run setup to create new structure
make setup

# Move existing data
[ -d swarmui/models ] && mv swarmui/models/* swarmui/data/Models/ 2>/dev/null; rmdir swarmui/models 2>/dev/null
[ -d swarmui/input ]  && mv swarmui/input/* swarmui/data/Inputs/ 2>/dev/null; rmdir swarmui/input 2>/dev/null
```

## Model Not Showing Up

1. Check file is in correct directory (ComfyUI: `comfyui/models/…`; SwarmUI: `swarmui/data/Models/`)
2. Verify file format (.safetensors, .ckpt, etc.)
3. Restart the service: `make restart`
4. Check file permissions

## Slow Performance

- Use .safetensors format (faster than .ckpt)
- Ensure models are on fast storage (SSD)
- Check GPU memory availability
- Close other GPU applications

## Corrupted Downloads

- Re-download the model
- Verify file size matches source
- Check disk space availability

## Kubernetes CrashLoopBackOff

### ComfyUI: "Found no NVIDIA driver"

**Symptom:** ComfyUI pod in CrashLoopBackOff; logs show:
`RuntimeError: Found no NVIDIA driver on your system`

**Cause:** ComfyUI assumes CUDA and calls `torch.cuda.current_device()` on startup. With `NVIDIA_VISIBLE_DEVICES=""` (CPU-only UI verification) or when the node has no GPU, it exits immediately.

**Options:**

1. **Use GPU nodes:** Re-enable NVIDIA env vars in `k8s/comfyui/deployment.yaml`, uncomment `nvidia.com/gpu` requests, and ensure the pod runs on a node with the NVIDIA device plugin and drivers.
2. **CPU-only verification:** ComfyUI cannot run without a GPU. To verify other UIs (SwarmUI, Ollama) without GPU, scale ComfyUI to 0:
   ```bash
   kubectl scale deployment/comfyui -n freya --replicas=0
   ```

### SwarmUI: "Failed to launch mode 'web'" or "webinstall"

**Symptom:** SwarmUI logs show:
`Failed to launch mode 'web' (If this is a headless/server install, run with '--launch_mode none')`  
or similar (e.g. `webinstall`).

**Cause:** Default launch mode tries to open a browser; in a headless Docker container or server there is no browser, so it fails.

**Fix (Docker):** The Freya image is built with `--launch_mode none` in the startup script. Rebuild the SwarmUI image and restart:
```bash
make build-swarmui
make down && make sui
```
(Or `make up` if you run all services.)

**Fix (Kubernetes):** The deployment must pass `--launch_mode none`. In `k8s/swarmui/deployment.yaml` the container should have:
```yaml
args: ["--launch_mode", "none"]
```
Then re-apply and restart: `kubectl apply -f k8s/swarmui/deployment.yaml && kubectl rollout restart deployment/swarmui -n freya`.

### SwarmUI: Extensions not persisting or UI not detecting them after reboot

**Symptom:** Extensions (e.g. MagicPrompt) disappear after container or host reboot, or the extension files are in `swarmui/extensions/` but the UI doesn't show the extension.

**Recommended: Use the UI for extensions.** Install entirely via **Server → Extensions → Install** in SwarmUI. SwarmUI downloads, compiles at startup, and persists the build to `swarmui/bin/`. No manual rebuild commands needed.

**Fix:** Run **`make setup`** (creates `swarmui/extensions` and `swarmui/bin`), then **`make sui`**. In SwarmUI use **Server → Extensions → Install**. SwarmUI builds extensions at startup; no `make swarmui-rebuild` needed.

**Extension still not showing?** SwarmUI builds extensions when it starts. If the build fails (e.g. "Could not locate assembly SwarmUI"), the extension project expects `SwarmUI.dll` at `src/bin/live_release/`. The Freya image provides a symlink fallback. Rebuild: **`make build-swarmui`** then **`make down && make sui`**. Run **`make swarmui-extensions-check`** to see build errors.


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
chmod +x scripts/check-gpu.sh
./scripts/check-gpu.sh
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
- Run only one service at a time
- Increase GPU memory or use multiple GPUs

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
2. **CPU-only verification:** ComfyUI cannot run without a GPU. To verify other UIs (SwarmUI, Open WebUI, Ollama) without GPU, scale ComfyUI to 0:
   ```bash
   kubectl scale deployment/comfyui -n freya --replicas=0
   ```

### SwarmUI: "Failed to launch mode 'webinstall'"

**Symptom:** SwarmUI pod starts then fails; logs show:
`Failed to launch mode 'webinstall' (If this is a headless/server install, run with '--launch_mode none')`

**Cause:** Default launch mode tries to open a browser; in a headless container that fails.

**Fix:** The deployment must pass `--launch_mode none`. In `k8s/swarmui/deployment.yaml` the container should have:
```yaml
args: ["--launch_mode", "none"]
```
Then re-apply and restart: `kubectl apply -f k8s/swarmui/deployment.yaml && kubectl rollout restart deployment/swarmui -n freya`.

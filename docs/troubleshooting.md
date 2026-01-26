# Troubleshooting Guide

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
docker run --rm --gpus all nvidia/cuda:12.1.0-base-ubuntu22.04 nvidia-smi
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

## Model Not Showing Up

1. Check file is in correct directory
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

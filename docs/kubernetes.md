# Kubernetes Deployment Guide

The project includes Kubernetes manifests for deploying ComfyUI and SwarmUI to a Kubernetes cluster.

## Prerequisites

- Kubernetes cluster with GPU nodes
- NVIDIA Device Plugin installed
- Persistent volumes configured
- Container registry to push images (or use local images)

## Build and Push Images

```bash
# Build images
make build

# Tag for your registry
docker tag freya-comfyui:latest your-registry/freya-comfyui:latest
docker tag freya-swarmui:latest your-registry/freya-swarmui:latest

# Push to registry
docker push your-registry/freya-comfyui:latest
docker push your-registry/freya-swarmui:latest
```

## Update Kubernetes Manifests

Edit `k8s/comfyui/deployment.yaml` and `k8s/swarmui/deployment.yaml` to use your registry:

```yaml
image: your-registry/freya-comfyui:latest
imagePullPolicy: Always
```

## Deploy to Kubernetes

**Option 1: Deploy script (prd-apps context)**  
From the repo root, run the script in `k8s/` to deploy to the prd-apps cluster (creates namespace, applies all manifests, waits for deployments):

```bash
./k8s/deploy-to-prd-apps.sh
```

**Option 2: Manual apply**

```bash
# Create namespace
kubectl create namespace freya

# Deploy ComfyUI
kubectl apply -f k8s/comfyui/

# Deploy SwarmUI
kubectl apply -f k8s/swarmui/

# Deploy Ollama (for SwarmUI MagicPrompt extension)
kubectl apply -f k8s/ollama/

# Optional: Deploy ingress
kubectl apply -f k8s/ingress.yaml

# Check status
kubectl get pods -n freya
kubectl get svc -n freya
```

## Access Services

After deployment, access services via:

### Ingress (if configured)
Update `k8s/ingress.yaml` with your domain

### Port Forwarding
```bash
kubectl port-forward -n freya svc/comfyui 8188:8188
kubectl port-forward -n freya svc/swarmui 7801:7801
kubectl port-forward -n freya svc/ollama 11434:11434  # Internal only (for debugging)
```

**Note:** Ollama is internal-only and not exposed via ingress. SwarmUI connects to Ollama via the Kubernetes service DNS name (`ollama.freya.svc.cluster.local:11434`).

### NodePort/LoadBalancer
Modify service type in manifests

## Environment Variables

Both services support GPU configuration via environment variables:
- `NVIDIA_VISIBLE_DEVICES`: GPU device IDs (default: `all`)
- `NVIDIA_DRIVER_CAPABILITIES`: Driver capabilities (default: `compute,utility`)

## Model Management

Models are stored in **PersistentVolumeClaims (PVCs)** that persist across pod restarts. The deployment manifests create PVCs for model storage:

### Storage Volumes

**ComfyUI:**
- `comfyui-models` (100Gi) → `/app/ComfyUI/models` - All model types (checkpoints, LoRA, VAE, ControlNet, etc.)
- `comfyui-output` (50Gi) → `/app/ComfyUI/output` - Generated images
- `comfyui-input` (10Gi) → `/app/ComfyUI/input` - Input images

**SwarmUI:**
- `swarmui-data` (130Gi) → `/app/SwarmUI/Data` - Main data directory (includes `Models/` subdirectory)
- `swarmui-swarmui-models` (50Gi) → `/app/SwarmUI/Models` - SwarmUI internal model storage
- `swarmui-output` (50Gi) → `/app/SwarmUI/Output` - Generated images
- `swarmui-dlbackend` (50Gi) → `/app/SwarmUI/dlbackend` - Download backend cache
- `swarmui-extensions` (10Gi) → `/app/SwarmUI/src/Extensions` - Extension source code (e.g. MagicPrompt)
- `swarmui-bin` (5Gi) → `/app/SwarmUI/src/bin` - Compiled app with extensions (persisted build output)

**Ollama:**
- `ollama-models` (50Gi) → `/root/.ollama` - Ollama model storage (for MagicPrompt extension)

### Adding Models

There are several ways to add models to Kubernetes deployments:

#### Method 1: Copy files into running pod (quick, one-off)

```bash
# Get pod name
POD=$(kubectl get pods -n freya -l app=comfyui -o jsonpath='{.items[0].metadata.name}')

# Copy a model file
kubectl cp /path/to/model.safetensors freya/$POD:/app/ComfyUI/models/checkpoints/

# For SwarmUI
POD=$(kubectl get pods -n freya -l app=swarmui -o jsonpath='{.items[0].metadata.name}')
kubectl cp /path/to/model.safetensors freya/$POD:/app/SwarmUI/Data/Models/
```

#### Method 2: Exec into pod and download (interactive)

```bash
# ComfyUI
kubectl exec -it -n freya deployment/comfyui -- /bin/bash

# Inside the pod, download models using wget/curl
# Or use SwarmUI's built-in downloader via the web UI
```

#### Method 3: Use SwarmUI's built-in downloader (recommended for SwarmUI)

1. Port-forward to SwarmUI: `kubectl port-forward -n freya svc/swarmui 7801:7801`
2. Open http://localhost:7801 in your browser
3. Use SwarmUI's model download utility (Server → Models or similar)
4. Models are saved to the persistent volume automatically

#### Method 4: Init Container or Job (automated, CI/CD)

Create a Kubernetes Job or use an init container to download models before the main container starts. Example Job:

```yaml
apiVersion: batch/v1
kind: Job
metadata:
  name: download-comfyui-models
  namespace: freya
spec:
  template:
    spec:
      containers:
      - name: downloader
        image: curlimages/curl:latest
        command:
        - sh
        - -c
        - |
          curl -L -o /models/checkpoints/model.safetensors \
            "https://huggingface.co/.../model.safetensors"
        volumeMounts:
        - name: models
          mountPath: /models
      volumes:
      - name: models
        persistentVolumeClaim:
          claimName: comfyui-models
      restartPolicy: Never
```

### Model Directory Structure

**ComfyUI** (in `comfyui-models` PVC):
```
/app/ComfyUI/models/
├── checkpoints/      # Main models (.safetensors, .ckpt)
├── loras/           # LoRA models
├── vae/             # VAE models
├── controlnet/      # ControlNet models
├── upscale_models/  # Upscaling models
├── embeddings/      # Textual inversions
└── ...
```

**SwarmUI** (in `swarmui-data` PVC):
```
/app/SwarmUI/Data/Models/   # Main model directory
/app/SwarmUI/Models/        # Internal model storage
```

See [MODELS.md](MODELS.md) for detailed directory structure and model types.

### Verifying Models

```bash
# List models in ComfyUI
kubectl exec -n freya deployment/comfyui -- ls -lh /app/ComfyUI/models/checkpoints/

# List models in SwarmUI
kubectl exec -n freya deployment/swarmui -- ls -lh /app/SwarmUI/Data/Models/

# Check PVC usage
kubectl get pvc -n freya
```

### Ollama Model Management

Ollama is included in the Kubernetes manifests. Models are stored in the `ollama-models` PVC mounted at `/root/.ollama`:

```bash
# Pull default model (dolphin3 — https://ollama.com/library/dolphin3)
kubectl exec -n freya deployment/ollama -- ollama pull dolphin3

# List models
kubectl exec -n freya deployment/ollama -- ollama list

# Remove a model
kubectl exec -n freya deployment/ollama -- ollama rm dolphin3
```

**Note:** Models persist across pod restarts as long as the PVC exists. To remove all models, delete the PVC (this deletes all data). 

**Configuring SwarmUI MagicPrompt with Ollama in Kubernetes:**

1. Port-forward to SwarmUI: `kubectl port-forward -n freya svc/swarmui 7801:7801`
2. In SwarmUI, go to **Server → Extensions** and install **MagicPrompt** (if not already installed)
3. Open **MagicPrompt** settings and configure:
   - **Chat Backend**: Ollama
   - **Base URL**: `http://ollama.freya.svc.cluster.local:11434` (internal Kubernetes service DNS)
   - **Chat Model**: Select an Ollama model you've pulled

**Note:** Ollama is internal-only (no ingress). SwarmUI connects via the Kubernetes service DNS name. Port-forwarding to Ollama (`kubectl port-forward -n freya svc/ollama 11434:11434`) is only needed for debugging or if you want to use `http://localhost:11434` from your local machine.

For Docker Compose usage, see [SwarmUI MagicPrompt guide](swarmui-magicprompt.md) for `make llm-pull`, `make llm-list`, and `make llm-rm` commands.

### Differences from Docker/Local Usage

- **No `make download-model`**: The Makefile commands (`make download-model`, `make llm-pull`, etc.) work with Docker Compose, not Kubernetes. Use the methods above instead.
- **PVCs vs local directories**: Models are in Kubernetes volumes, not local `./comfyui/models/` directories.
- **Access**: You can't directly access model files from your local machine—use `kubectl exec`, `kubectl cp`, or port-forward to the UI.
- **Storage size**: Adjust PVC sizes in the deployment manifests if you need more space (e.g., change `storage: 100Gi` to `storage: 500Gi`).

### Troubleshooting

**Models not showing up:**
1. Check the pod is running: `kubectl get pods -n freya`
2. Verify files are in the correct directory: `kubectl exec -n freya deployment/comfyui -- ls /app/ComfyUI/models/checkpoints/`
3. Restart the pod: `kubectl rollout restart deployment/comfyui -n freya`
4. Check PVC is mounted: `kubectl describe pod -n freya -l app=comfyui | grep -A 5 Mounts`

**Out of storage:**
- Check PVC usage: `kubectl get pvc -n freya`
- Increase PVC size (requires storage class that supports expansion) or create a new larger PVC and migrate data

For more details on model types, sources, and formats, see [MODELS.md](MODELS.md).

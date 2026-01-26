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

```bash
# Create namespace
kubectl create namespace freya

# Deploy ComfyUI
kubectl apply -f k8s/comfyui/

# Deploy SwarmUI
kubectl apply -f k8s/swarmui/

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
```

### NodePort/LoadBalancer
Modify service type in manifests

## Environment Variables

Both services support GPU configuration via environment variables:
- `NVIDIA_VISIBLE_DEVICES`: GPU device IDs (default: `all`)
- `NVIDIA_DRIVER_CAPABILITIES`: Driver capabilities (default: `compute,utility`)

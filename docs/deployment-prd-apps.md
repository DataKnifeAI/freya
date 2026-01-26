# Deploying Freya to prd-apps Cluster

This guide covers deploying Freya (ComfyUI & SwarmUI) to the prd-apps Kubernetes cluster.

## Prerequisites

- Access to `prd-apps` Kubernetes cluster
- `kubectl` configured with `prd-apps` context
- Images available in Harbor registry: `harbor.dataknife.net/tools/freya-*`
- Persistent volume storage available

## Current Status

### Namespace
- **Namespace**: `freya`
- **Status**: Created and ready

### Images Required
- `harbor.dataknife.net/tools/freya-comfyui:latest`
- `harbor.dataknife.net/tools/freya-swarmui:latest`

**Note**: Images are built and pushed by GitLab CI/CD pipeline. Ensure images exist in Harbor before deployment.

## Deployment Steps

### 1. Validate Manifests

```bash
# Validate all manifests
kubectl --context=prd-apps apply --dry-run -f k8s/comfyui/deployment.yaml
kubectl --context=prd-apps apply --dry-run -f k8s/swarmui/deployment.yaml
kubectl --context=prd-apps apply --dry-run -f k8s/ingress.yaml
```

### 2. Deploy Using Script

```bash
./scripts/deploy-to-prd-apps.sh
```

### 3. Manual Deployment

```bash
# Set context
kubectl config use-context prd-apps

# Create namespace (if not exists)
kubectl create namespace freya

# Deploy ComfyUI
kubectl apply -f k8s/comfyui/deployment.yaml

# Deploy SwarmUI
kubectl apply -f k8s/swarmui/deployment.yaml

# Deploy Ingress
kubectl apply -f k8s/ingress.yaml
```

## GPU Configuration

**Current Status**: GPU resources are **commented out** in the manifests for testing without NVIDIA.

To enable GPU support (when NVIDIA is available):

1. Uncomment the `resources` section in both deployment files
2. Ensure NVIDIA Device Plugin is installed in the cluster
3. Ensure nodes have GPU resources available

## Verification

### Check Pod Status

```bash
kubectl get pods -n freya
kubectl describe pod -n freya -l app=comfyui
kubectl describe pod -n freya -l app=swarmui
```

### Check Services

```bash
kubectl get svc -n freya
```

### Check Persistent Volumes

```bash
kubectl get pvc -n freya
```

### Check Ingress

```bash
kubectl get ingress -n freya
```

### View Logs

```bash
# ComfyUI logs
kubectl logs -n freya -l app=comfyui --tail=50

# SwarmUI logs
kubectl logs -n freya -l app=swarmui --tail=50
```

## Accessing the UIs

### Port Forwarding (for testing)

```bash
# ComfyUI
kubectl port-forward -n freya svc/comfyui 8188:8188

# SwarmUI
kubectl port-forward -n freya svc/swarmui 7801:7801
```

Then access:
- ComfyUI: http://localhost:8188
- SwarmUI: http://localhost:7801

### Via Ingress

Once ingress is configured and DNS is set up:
- ComfyUI: http://comfyui.dataknife.net
- SwarmUI: http://swarmui.dataknife.net

## Troubleshooting

### Pods Not Starting

1. **Check image pull errors**:
   ```bash
   kubectl describe pod -n freya <pod-name>
   ```
   Look for `ImagePullBackOff` or `ErrImagePull` errors.

2. **Verify images exist in Harbor**:
   - Check Harbor registry: `harbor.dataknife.net/tools/`
   - Ensure images are pushed with correct tags

3. **Check resource constraints**:
   ```bash
   kubectl top pods -n freya
   kubectl describe nodes
   ```

### Persistent Volume Issues

```bash
# Check PVC status
kubectl get pvc -n freya

# Check PV status
kubectl get pv

# Describe PVC for details
kubectl describe pvc -n freya <pvc-name>
```

### GPU Issues (when enabled)

1. **Verify NVIDIA Device Plugin**:
   ```bash
   kubectl get pods -n kube-system | grep nvidia
   ```

2. **Check node GPU resources**:
   ```bash
   kubectl describe node <node-name> | grep -i gpu
   ```

3. **Verify GPU requests in deployment**:
   - Ensure `nvidia.com/gpu` resources are uncommented
   - Check node has available GPU resources

## Current Configuration

### Namespace
- All resources use namespace: `freya`

### Image Registry
- Harbor: `harbor.dataknife.net/tools/`
- Images: `freya-comfyui:latest`, `freya-swarmui:latest`

### Storage
- ComfyUI: 100Gi models, 50Gi output, 10Gi input
- SwarmUI: 100Gi models, 50Gi output, 10Gi input, 20Gi data, 50Gi dlbackend

### GPU
- **Currently disabled** for testing without NVIDIA
- Can be enabled by uncommenting resources section

## Next Steps

1. ✅ Validate YAML manifests
2. ⏳ Build and push images to Harbor (via GitLab CI)
3. ⏳ Deploy to prd-apps cluster
4. ⏳ Verify pods start successfully
5. ⏳ Test UI access via port-forward
6. ⏳ Configure ingress and DNS
7. ⏳ Enable GPU support when NVIDIA is available

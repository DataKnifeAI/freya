#!/bin/bash
# Deploy Freya to prd-apps cluster
# This script deploys ComfyUI and SwarmUI to the prd-apps Kubernetes cluster

set -e

CONTEXT="prd-apps"
NAMESPACE="freya"

echo "🚀 Deploying Freya to prd-apps cluster..."

# Check kubectl context
if ! kubectl config get-contexts | grep -q "$CONTEXT"; then
    echo "❌ Error: kubectl context '$CONTEXT' not found"
    exit 1
fi

# Set context
kubectl config use-context "$CONTEXT"

# Create namespace if it doesn't exist
echo "📦 Creating namespace '$NAMESPACE'..."
kubectl create namespace "$NAMESPACE" --dry-run=client -o yaml | kubectl apply -f -

# Deploy ComfyUI
echo "🎨 Deploying ComfyUI..."
kubectl apply -f k8s/comfyui/deployment.yaml

# Deploy SwarmUI
echo "🐝 Deploying SwarmUI..."
kubectl apply -f k8s/swarmui/deployment.yaml

# Deploy Ingress (optional)
echo "🌐 Deploying Ingress..."
kubectl apply -f k8s/ingress.yaml

# Wait for deployments
echo "⏳ Waiting for deployments to be ready..."
kubectl wait --for=condition=available --timeout=300s deployment/comfyui -n "$NAMESPACE" || echo "⚠️  ComfyUI deployment not ready yet"
kubectl wait --for=condition=available --timeout=300s deployment/swarmui -n "$NAMESPACE" || echo "⚠️  SwarmUI deployment not ready yet"

# Show status
echo ""
echo "📊 Deployment Status:"
echo "===================="
kubectl get pods -n "$NAMESPACE"
echo ""
kubectl get svc -n "$NAMESPACE"
echo ""
kubectl get ingress -n "$NAMESPACE"

echo ""
echo "✅ Deployment complete!"
echo ""
echo "To access services:"
echo "  ComfyUI:  kubectl port-forward -n $NAMESPACE svc/comfyui 8188:8188"
echo "  SwarmUI:  kubectl port-forward -n $NAMESPACE svc/swarmui 7801:7801"
echo ""
echo "Or via Ingress:"
echo "  ComfyUI:  http://comfyui.dataknife.net"
echo "  SwarmUI:  http://swarmui.dataknife.net"

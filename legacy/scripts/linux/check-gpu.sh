#!/bin/bash
# GPU Diagnostic Script - Check NVIDIA GPU access in Docker

set -e

echo "=== GPU Diagnostic Check ==="
echo ""

echo "1. Checking host GPU..."
if command -v nvidia-smi &> /dev/null; then
    echo "✓ nvidia-smi found"
    nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv,noheader
else
    echo "✗ nvidia-smi not found - NVIDIA drivers may not be installed"
    exit 1
fi

echo ""
echo "2. Checking Docker NVIDIA runtime..."
if docker info 2>&1 | grep -qi nvidia; then
    echo "✓ NVIDIA runtime found in Docker"
    docker info 2>&1 | grep -i nvidia
else
    echo "✗ NVIDIA runtime NOT found in Docker"
    echo ""
    echo "Install NVIDIA Container Toolkit for your distribution."
    echo "  See docs/installation.md (Ubuntu, Debian, Fedora, RHEL, openSUSE, Arch)"
    echo "  Or run: sudo ./scripts/linux/install-nvidia-toolkit.sh (supported distros only)"
    echo ""
    exit 1
fi

echo ""
echo "3. Testing GPU access in Docker..."
if docker run --rm --gpus all nvidia/cuda:13.1.1-base-ubuntu24.04 nvidia-smi &> /dev/null; then
    echo "✓ GPU accessible in Docker containers"
    docker run --rm --gpus all nvidia/cuda:13.1.1-base-ubuntu24.04 nvidia-smi --query-gpu=name,memory.total --format=csv,noheader
else
    echo "✗ GPU NOT accessible in Docker containers"
    echo ""
    echo "Troubleshooting:"
    echo "  1. Ensure NVIDIA Container Toolkit is installed (see above)"
    echo "  2. Restart Docker: sudo systemctl restart docker"
    echo "  3. Verify: docker run --rm --gpus all nvidia/cuda:13.1.1-base-ubuntu24.04 nvidia-smi"
    echo ""
    exit 1
fi

echo ""
echo "4. Checking docker-compose GPU configuration..."
if docker compose config 2>&1 | grep -q "nvidia"; then
    echo "✓ docker-compose.yml has GPU configuration"
else
    echo "✗ docker-compose.yml missing GPU configuration"
fi

echo ""
echo "=== Diagnostic Complete ==="
echo ""
echo "If all checks passed, you can run:"
echo "  make build"
echo "  make up"
echo "  make check-gpu-comfyui"

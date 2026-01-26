#!/bin/bash
# Install NVIDIA Container Toolkit for Arch Linux / CachyOS

set -e

echo "=== Installing NVIDIA Container Toolkit ==="
echo ""

# Check if running as root or with sudo
if [ "$EUID" -ne 0 ]; then 
    echo "This script needs sudo privileges."
    echo "Please run: sudo $0"
    exit 1
fi

echo "Detected: CachyOS (Arch-based)"
echo ""

# Install nvidia-container-toolkit
echo "Installing nvidia-container-toolkit..."
pacman -S --noconfirm nvidia-container-toolkit

echo ""
echo "Configuring Docker to use NVIDIA runtime..."

# Configure Docker
nvidia-ctk runtime configure --runtime=docker

echo ""
echo "Restarting Docker..."
systemctl restart docker

echo ""
echo "✓ Installation complete!"
echo ""
echo "Verifying installation..."
if docker run --rm --gpus all nvidia/cuda:12.1.0-base-ubuntu22.04 nvidia-smi &> /dev/null; then
    echo "✓ GPU accessible in Docker!"
    docker run --rm --gpus all nvidia/cuda:12.1.0-base-ubuntu22.04 nvidia-smi --query-gpu=name,memory.total --format=csv,noheader
else
    echo "✗ GPU still not accessible. Please check:"
    echo "  1. Docker service is running: systemctl status docker"
    echo "  2. NVIDIA drivers are installed: nvidia-smi"
    echo "  3. Try: docker run --rm --gpus all nvidia/cuda:12.1.0-base-ubuntu22.04 nvidia-smi"
fi

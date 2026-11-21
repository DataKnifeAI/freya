#!/bin/bash
# Install NVIDIA Container Toolkit on supported Linux distributions.
# Supported: Ubuntu, Debian, Fedora, RHEL, CentOS, openSUSE, Arch Linux (and derivatives).
# Run with: sudo ./scripts/linux/install-nvidia-toolkit.sh

set -e

echo "=== Installing NVIDIA Container Toolkit ==="
echo ""

if [ "$EUID" -ne 0 ]; then
    echo "This script needs sudo privileges."
    echo "Please run: sudo $0"
    exit 1
fi

# Detect distribution
if [ -f /etc/os-release ]; then
    . /etc/os-release
    DISTRO_ID="${ID:-unknown}"
    DISTRO_VERSION_ID="${VERSION_ID:-}"
else
    DISTRO_ID="unknown"
fi

echo "Detected: ${PRETTY_NAME:-$DISTRO_ID} (ID=$DISTRO_ID)"
echo ""

install_ubuntu_debian() {
    distribution="${DISTRO_ID}${DISTRO_VERSION_ID}"
    curl -s -L https://nvidia.github.io/nvidia-docker/gpgkey | gpg --dearmor -o /usr/share/keyrings/nvidia-docker.gpg
    curl -s -L "https://nvidia.github.io/nvidia-docker/${distribution}/nvidia-docker.list" | \
        sed 's#deb https://#deb [signed-by=/usr/share/keyrings/nvidia-docker.gpg] https://#' | \
        tee /etc/apt/sources.list.d/nvidia-docker.list
    apt-get update && apt-get install -y nvidia-container-toolkit
}

install_fedora_rhel() {
    curl -s -L https://nvidia.github.io/libnvidia-container/stable/rpm/nvidia-container-toolkit.repo | \
        tee /etc/yum.repos.d/nvidia-container-toolkit.repo
    (command -v dnf >/dev/null 2>&1 && dnf install -y nvidia-container-toolkit) || \
        yum install -y nvidia-container-toolkit
}

install_opensuse() {
    distribution="${DISTRO_ID}${DISTRO_VERSION_ID}"
    zypper ar "https://nvidia.github.io/nvidia-docker/${distribution}/nvidia-docker.repo"
    zypper ref && zypper in -y nvidia-container-toolkit
}

install_arch() {
    pacman -S --noconfirm nvidia-container-toolkit
}

case "$DISTRO_ID" in
    ubuntu|debian)
        install_ubuntu_debian
        ;;
    fedora|rhel|centos|rocky|almalinux)
        install_fedora_rhel
        ;;
    opensuse*|sles)
        install_opensuse
        ;;
    arch|cachyos|endeavouros)
        install_arch
        ;;
    *)
        echo "Unsupported distribution: $DISTRO_ID"
        echo "See docs/installation.md for manual install (Ubuntu, Debian, Fedora, RHEL, openSUSE, Arch)."
        exit 1
        ;;
esac

echo ""
echo "Configuring Docker to use NVIDIA runtime..."
nvidia-ctk runtime configure --runtime=docker

echo ""
echo "Restarting Docker..."
systemctl restart docker

echo ""
echo "✓ Installation complete!"
echo ""
echo "Verifying installation..."
if docker run --rm --gpus all nvidia/cuda:13.1.1-base-ubuntu24.04 nvidia-smi &>/dev/null; then
    echo "✓ GPU accessible in Docker!"
    docker run --rm --gpus all nvidia/cuda:13.1.1-base-ubuntu24.04 nvidia-smi --query-gpu=name,memory.total --format=csv,noheader
else
    echo "✗ GPU still not accessible. Please check:"
    echo "  1. Docker service is running: systemctl status docker"
    echo "  2. NVIDIA drivers are installed: nvidia-smi"
    echo "  3. Try: docker run --rm --gpus all nvidia/cuda:13.1.1-base-ubuntu24.04 nvidia-smi"
fi

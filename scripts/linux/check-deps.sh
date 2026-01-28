#!/bin/bash
# Check Freya dependencies (Docker, Docker Compose, Git, NVIDIA driver/toolkit).
# Install Git on Linux when possible; otherwise print install instructions.
# Supported platforms: Linux, Windows (WSL2). NVIDIA support only.

set -e

INSTALL_DOC="docs/installation.md"
FAILED=0

echo "=== Checking Freya dependencies ==="
echo ""

# --- Docker ---
if ! command -v docker &>/dev/null; then
    echo "✗ Docker is not installed or not in PATH."
    echo "  Install:"
    echo "    Linux:   https://docs.docker.com/engine/install/"
    echo "    Windows: Use WSL2 and install Docker Desktop (WSL2 backend), or install Docker Engine inside WSL2."
    echo "  See $INSTALL_DOC (Checking and installing Docker)."
    FAILED=1
else
    echo "✓ Docker found: $(docker --version)"
fi

# --- Docker Compose v2 ---
if ! docker compose version &>/dev/null 2>&1; then
    echo "✗ Docker Compose v2 is not available (run: docker compose version)."
    echo "  Install Docker Compose v2 (plugin): https://docs.docker.com/compose/install/"
    echo "  See $INSTALL_DOC (Checking and installing Docker)."
    FAILED=1
else
    echo "✓ Docker Compose found: $(docker compose version --short 2>/dev/null || docker compose version)"
fi

# --- Git ---
if ! command -v git &>/dev/null; then
    echo "✗ Git is not installed or not in PATH."
    if [ -f /etc/os-release ]; then
        . /etc/os-release
        echo "  Attempting to install Git..."
        if command -v apt-get &>/dev/null; then
            sudo apt-get update -qq && sudo apt-get install -y git && echo "✓ Git installed (apt)." || true
        elif command -v dnf &>/dev/null; then
            sudo dnf install -y git && echo "✓ Git installed (dnf)." || true
        elif command -v zypper &>/dev/null; then
            sudo zypper in -y git && echo "✓ Git installed (zypper)." || true
        elif command -v pacman &>/dev/null; then
            sudo pacman -S --noconfirm git && echo "✓ Git installed (pacman)." || true
        fi
    fi
    if ! command -v git &>/dev/null; then
        echo "  Install Git:"
        echo "    Linux:   apt install git / dnf install git / pacman -S git / zypper in git"
        echo "    Windows: Install Git for Windows or use WSL2 (git in Linux distro)."
        echo "  See $INSTALL_DOC."
        FAILED=1
    fi
else
    echo "✓ Git found: $(git --version)"
fi

# --- NVIDIA driver (Linux; warn only) ---
if [ -f /etc/os-release ]; then
    if ! command -v nvidia-smi &>/dev/null; then
        echo ""
        echo "⚠ NVIDIA driver not detected (nvidia-smi not found)."
        echo "  GPU acceleration requires an NVIDIA driver. Install:"
        echo "    Linux:   Use your distro's package manager or https://www.nvidia.com/Download/index.aspx"
        echo "    Windows: Install NVIDIA driver for Windows (WSL2 uses host driver)."
        echo "  See $INSTALL_DOC and docs/gpu-compatibility.md."
    else
        echo "✓ NVIDIA driver: $(nvidia-smi --query-gpu=driver_version --format=csv,noheader 2>/dev/null | head -1 || echo 'detected')"
    fi

    # --- NVIDIA Container Toolkit (warn only) ---
    if command -v docker &>/dev/null && docker info 2>&1 | grep -qi nvidia; then
        echo "✓ Docker has NVIDIA runtime."
    else
        if command -v docker &>/dev/null; then
            echo ""
            echo "⚠ Docker does not report NVIDIA runtime (GPU in containers may not work)."
            echo "  Install NVIDIA Container Toolkit:"
            echo "    Linux:   sudo ./scripts/linux/install-nvidia-toolkit.sh  (or see $INSTALL_DOC)"
            echo "    Windows: Install toolkit inside your WSL2 distro if using GPU in containers."
        fi
    fi
fi

echo ""

if [ "$FAILED" -eq 1 ]; then
    echo "Install missing dependencies above, then run: make setup"
    echo "Full instructions: $INSTALL_DOC"
    exit 1
fi

echo "✓ All required dependencies are present."
exit 0

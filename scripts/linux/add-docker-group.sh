#!/bin/bash
# Create docker group if missing, add current user to it.
# Run with: make fix-docker-group  (or ./scripts/linux/add-docker-group.sh)

set -e

CUR_USER=${SUDO_USER:-$USER}

# Already in docker group? Just remind about newgrp
if groups "$CUR_USER" 2>/dev/null | grep -q '\bdocker\b'; then
    echo "✓ User $CUR_USER is already in the docker group."
    echo "  If Docker still fails, run: newgrp docker"
    echo "  (or log out and back in)"
    exit 0
fi

# Need sudo but no TTY? Print manual commands
if ! [ -t 0 ] || ! sudo -n true 2>/dev/null; then
    echo "This script needs sudo. Run these commands in your terminal:"
    echo ""
    echo "  sudo groupadd docker 2>/dev/null || true"
    echo "  sudo usermod -aG docker $CUR_USER"
    echo "  newgrp docker"
    echo ""
    exit 1
fi

# Create docker group if it doesn't exist (Docker install usually does this)
if ! getent group docker &>/dev/null; then
    echo "Creating docker group..."
    sudo groupadd docker
fi

# Add current user to docker group
echo "Adding $CUR_USER to docker group..."
sudo usermod -aG docker "$CUR_USER"

echo ""
echo "✓ Done. For group membership to take effect:"
echo "  • Log out and back in, or"
echo "  • Run: newgrp docker"

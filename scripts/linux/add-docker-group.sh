#!/bin/bash
# Create docker group if missing, add current user to it.
# Run with: make fix-docker-group  (or ./scripts/linux/add-docker-group.sh)

set -e

# Create docker group if it doesn't exist (Docker install usually does this)
if ! getent group docker &>/dev/null; then
    echo "Creating docker group..."
    sudo groupadd docker
fi

# Add current user to docker group
CUR_USER=${SUDO_USER:-$USER}
echo "Adding $CUR_USER to docker group..."
sudo usermod -aG docker "$CUR_USER"

echo ""
echo "✓ Done. For group membership to take effect:"
echo "  • Log out and back in, or"
echo "  • Run: newgrp docker"

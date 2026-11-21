#!/usr/bin/env bash
# Point freya.dataknife.net at this machine's kind Ingress (ports 80/443).
# Public DNS CNAMEs freya → prd-apps Traefik, which returns plain-text:
#   404 page not found
set -euo pipefail

IP="${FREYA_HOSTS_IP:-127.0.0.1}"
LINE="${IP} freya.dataknife.net"

apply() {
  local hosts_file=/etc/hosts
  grep -vE '(^|[[:space:]])freya\.dataknife\.net([[:space:]]|$)' "$hosts_file" \
    | grep -v '^# Freya kind-local' > /tmp/hosts.freya.new || true
  {
    cat /tmp/hosts.freya.new
    echo ""
    echo "# Freya kind-local"
    echo "$LINE"
  } > "$hosts_file"
  rm -f /tmp/hosts.freya.new
  echo "Updated $hosts_file → $LINE"
}

if [[ -w /etc/hosts ]]; then
  apply
elif command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
  # docker group can rewrite /etc/hosts without sudo
  docker run --rm -e LINE="$LINE" -v /etc/hosts:/etc/hosts alpine sh -c '
    grep -vE "(^|[[:space:]])freya\\.dataknife\\.net([[:space:]]|$)" /etc/hosts \
      | grep -v "^# Freya kind-local" > /tmp/hosts.new || true
    { cat /tmp/hosts.new; echo; echo "# Freya kind-local"; echo "$LINE"; } > /etc/hosts
  '
  echo "Updated /etc/hosts → $LINE (via docker)"
else
  echo "Need write access to /etc/hosts. Re-run with sudo:" >&2
  echo "  sudo $0" >&2
  exit 1
fi

# nsswitch uses systemd-resolved before files; resolved synthesizes from /etc/hosts
# but a stale DNS answer can linger until flush.
if command -v resolvectl >/dev/null 2>&1; then
  resolvectl flush-caches 2>/dev/null || true
fi

grep -E 'freya\.dataknife\.net|Freya kind-local' /etc/hosts || true
echo
echo "Verify: curl -sk -o /dev/null -w '%{http_code}\\n' https://freya.dataknife.net/  # expect 200"
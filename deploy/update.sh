#!/bin/sh
set -eu
cd "$(dirname "$0")"

git -C .. pull --ff-only
podman-compose build trex

for container in trex_cloudflared_1 trex_trex_1; do
  if podman container exists "$container"; then podman rm -f "$container" >/dev/null; fi
done
if grep -q '^CLOUDFLARE_TUNNEL_TOKEN=.' .env; then
  podman-compose --profile tunnel up -d
else
  podman-compose up -d
fi

for _ in $(seq 1 30); do
  if curl -fsS -o /dev/null http://127.0.0.1:8080/health; then
    echo "trex is up on $(podman inspect trex_trex_1 --format '{{.Image}}' | cut -c1-12)"
    exit 0
  fi
  sleep 2
done
echo "trex didn't answer its health check; see podman-compose logs trex" >&2
exit 1

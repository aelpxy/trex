# Deploying trex

One host runs trex, Postgres 18 and Valkey with rootless Podman and `podman-compose`, next to the OpenShell gateway and the model proxy trex already uses. trex shares the host's network so it reaches the gateway (`127.0.0.1:17670`) and the model proxy on loopback; Postgres and Valkey listen on the host's loopback only.

## Layout on the host

`~/projects/trex` holds the source. `deploy/` beside this file holds what never goes in git:

| Path | What |
| --- | --- |
| `deploy/.env` | Database and Valkey passwords and trex's settings, from `.env.example` (`chmod 600`) |
| `deploy/trex.toml` | The model catalog with provider keys (`chmod 600`) |
| `deploy/certs/openshell/` | `ca.crt`, `tls.crt`, `tls.key` for the gateway |
| `deploy/data/library/` | Users' library files |

Postgres and Valkey data live in the `trex_postgres` and `trex_valkey` volumes.

## First start

```sh
cd ~/projects/trex/deploy
podman-compose build trex
podman-compose up -d
podman-compose ps
```

The containers restart on failure and come back after a reboot through `podman-restart.service`, which needs lingering for the user (`loginctl enable-linger`).

Open the ports to your network (Fedora Server's firewall only allows SSH by default):

```sh
sudo firewall-cmd --permanent --add-port=8080/tcp --add-port=8081/tcp && sudo firewall-cmd --reload
```

Sign up at `http://<host>:8080`, then make yourself an admin:

```sh
podman exec trex_trex_1 trex admin grant you@example.com
```

Previews are served at `TREX_PREVIEW_URL`. On a home network, `http://{id}.preview.<host ip>.nip.io:8081` works without any DNS setup, because nip.io resolves names that contain an IP to that IP.

## Updating

From the development machine, copy the source over and rebuild:

```sh
tar -cz --exclude=./.git --exclude=./target --exclude=./frontend/node_modules --exclude=./frontend/build \
  --exclude=./data --exclude=./certs --exclude=./.env --exclude=./trex.toml \
  --exclude=./deploy/.env --exclude=./deploy/trex.toml --exclude=./deploy/certs --exclude=./deploy/data . \
  | ssh <host> 'cd ~/projects/trex && tar -xz'
ssh <host> 'cd ~/projects/trex/deploy && podman-compose build trex && podman-compose up -d trex'
```

Migrations run when trex starts. A new sandbox image is built separately on the same host (`podman build -t localhost/trex-sandbox:latest images/sandbox`) and only reaches new chats.

## Day to day

```sh
podman-compose logs -f trex                       # json logs
podman exec trex_postgres_1 pg_dump -U trex trex | gzip > trex-$(date +%F).sql.gz   # backup
gunzip -c trex-DATE.sql.gz | podman exec -i trex_postgres_1 psql -U trex trex       # restore into an empty database
```

Back up `deploy/data/library` with the database; attachments and library files live there.

## Serving it on the internet

Behind CGNAT nothing can connect in, so use an outbound tunnel. With a Cloudflare Tunnel, point a hostname at `http://localhost:8080` and a wildcard on a separate domain at `http://localhost:8081` (one subdomain level, so the free certificate covers it), then in `.env`:

```sh
TREX_ADDR=127.0.0.1:8080
TREX_PREVIEW_ADDR=127.0.0.1:8081
TREX_PREVIEW_URL=https://{id}.your-preview-domain.net
TREX_INSECURE_COOKIES=false
TREX_TRUST_PROXY_HEADERS=true
```

plus `CLOUDFLARE_TUNNEL_TOKEN` from the tunnel's page, then `podman-compose --profile tunnel up -d`, which also starts the `cloudflared` service. Listening on loopback only is what makes trusting the tunnel's forwarded headers safe, and nothing needs opening in the firewall. Turn on Always Use HTTPS for both domains.

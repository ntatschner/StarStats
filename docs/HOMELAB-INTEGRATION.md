# Homelab Integration Runbook

End-to-end bring-up for StarStats on the **voyager** docker host
(`D:\git\home-servers-build\compose\voyager\voyager-compose.yml`).

This document is the single source of truth for going from a clean
checkout to a running stack. If you change service config, update
this doc.

## Stack overview

| Layer | Component | Reused / New | Subdomain |
|---|---|---|---|
| Reverse proxy | Traefik | reused | (existing) |
| Identity (authn) | StarStats web (self-hosted; email/pw + OAuth link) | new | `app.example.com` |
| Identity (authz) | SpiceDB | new | (internal only) |
| API | starstats-api (Rust + Axum) | new | `api.example.com` |
| Web | starstats-web (Next.js) | new | `app.example.com` |
| Database | Postgres pgvector/pg17 | reused | (internal) |
| Cache / queue | Redis | reused | (internal) |
| Object storage | MinIO | new | `s3.example.com` + `minio.example.com` |
| Errors | GlitchTip | new | `errors.example.com` |
| Logs | Loki | new | (Grafana datasource) |
| Traces | Tempo | new | (Grafana datasource) |
| Metrics | Prometheus | new | (Grafana datasource) |
| Telemetry ingest | OTEL Collector | new | (internal only) |
| Dashboards | Grafana | reused | `grafana.example.com` |

## One-time host setup

### 1. Copy infra configs from repo to docker host

The compose file expects these paths:

```
$DOCKERDIR/starstats/init/init-databases.sh
$DOCKERDIR/starstats/spicedb/schema.zed              (applied via zed CLI)
$DOCKERDIR/starstats/otel-collector/config.yaml
$DOCKERDIR/starstats/loki/config.yaml
$DOCKERDIR/starstats/tempo/config.yaml
$DOCKERDIR/starstats/prometheus/prometheus.yml
```

Sync from the repo's `infra/` directory:

```bash
# On the docker host, run once at install and after any infra/ change:
rsync -av --delete \
  /path/to/StarStats/infra/ \
  ${DOCKERDIR}/starstats/
chmod +x ${DOCKERDIR}/starstats/init/init-databases.sh
```

For a Windows host, mirror with `robocopy /MIR <src> <dest>`.

### 2. Create secret files

Create one file per secret in `$SECRETSDIR`. Generate strong values:

```bash
# 32-byte hex passwords / keys — use openssl, /dev/urandom, or a password manager
gen() { openssl rand -hex 32; }

gen > ${SECRETSDIR}/starstats_db_password
gen > ${SECRETSDIR}/spicedb_db_password
gen > ${SECRETSDIR}/glitchtip_db_password
gen > ${SECRETSDIR}/spicedb_preshared_key
gen > ${SECRETSDIR}/starstats_minio_root_password
gen > ${SECRETSDIR}/starstats_session_key
gen > ${SECRETSDIR}/glitchtip_secret_key

chmod 600 ${SECRETSDIR}/starstats_* ${SECRETSDIR}/spicedb_* ${SECRETSDIR}/glitchtip_*
```

The StarStats API mints its own JWTs from an RSA keypair. The
keypair lives outside `$SECRETSDIR` because the server *generates*
it on first boot (Docker secrets are read-only). Provision a
writable mount instead:

```bash
mkdir -p ${DOCKERDIR}/starstats/api-state
chmod 700 ${DOCKERDIR}/starstats/api-state
```

Mount this directory at `/var/lib/starstats` in the api service.
The first boot writes `jwt-key.pem` (mode 0600); back it up like
any other crypto material.

### 3. Configure StarStats JWT issuer

The API verifies tokens it minted itself, so the only knobs are
the canonical issuer URL and the audience:

```yaml
# voyager-compose.yml — starstats-api environment
STARSTATS_JWT_ISSUER: https://app.example.com
STARSTATS_JWT_AUDIENCE: starstats
STARSTATS_JWT_KEY_FILE: /var/lib/starstats/jwt-key.pem
```

`STARSTATS_JWT_ISSUER` must match the public origin of the web
app — the desktop client and any third-party verifier discover the
JWKS at `${STARSTATS_JWT_ISSUER}/.well-known/jwks.json` (Slice 5).

Web sign-up / sign-in is exposed at `https://app.example.com/auth/*`.
OAuth account linking (GitHub, Discord) is configured in the web
app's environment, not in the API.

### 4. Build and push application images

After Phase 0 + Phase 2 implementation lands:

```bash
# In the StarStats repo root
docker build -t registry.example.com/starstats/api:latest -f crates/starstats-server/Dockerfile .
docker build -t registry.example.com/starstats/web:latest -f apps/web/Dockerfile .

docker push registry.example.com/starstats/api:latest
docker push registry.example.com/starstats/web:latest
```

CI automates this on every tag push (`v*`) and via manual
`workflow_dispatch` — see `.github/workflows/release.yml`. The pipeline
publishes both application images plus thin "config" images for
`db-init`, `loki`, `tempo`, `prometheus`, and `otel-collector` so the
homelab pulls fully-versioned artefacts instead of bind-mounting
configs from a host directory.

## Compose service definitions

Add the following to `voyager-compose.yml`. Pieces to adapt before
copy-paste:

- `${REGISTRY}` — your private registry (e.g. `registry.example.com`).
- `${STARSTATS_TAG}` — image tag, defaults to `latest`. Pin to a
  specific `v1.2.3` in production once tagged releases are flowing.
- Traefik entrypoints / certresolvers — match the names already in
  use in your file (commonly `websecure` + `letsencrypt`).
- Network name (`voyager_t2_proxy`) — already declared at the top of
  voyager-compose.yml; just reference it from each service.

```yaml
# ----- Secrets ----------------------------------------------------
# Add to the top-level `secrets:` block. The files are created in
# step 2 of "One-time host setup".
secrets:
  starstats_db_password:
    file: ${SECRETSDIR}/starstats_db_password
  spicedb_db_password:
    file: ${SECRETSDIR}/spicedb_db_password
  glitchtip_db_password:
    file: ${SECRETSDIR}/glitchtip_db_password
  spicedb_preshared_key:
    file: ${SECRETSDIR}/spicedb_preshared_key
  starstats_minio_root_password:
    file: ${SECRETSDIR}/starstats_minio_root_password
  starstats_session_key:
    file: ${SECRETSDIR}/starstats_session_key
  glitchtip_secret_key:
    file: ${SECRETSDIR}/glitchtip_secret_key
  # Reused: existing postgres superuser secret already declared.
  # postgres_default: { file: ${SECRETSDIR}/postgres_default }

# ----- Volumes ----------------------------------------------------
volumes:
  starstats_minio_data:
  starstats_api_state:    # JWT key + any future writable state
  loki_data:
  tempo_data:
  prometheus_data:

# ----- Services ---------------------------------------------------
services:

  # --- One-shot: provision DB roles, databases, extensions --------
  starstats-db-init:
    image: ${REGISTRY}/starstats/db-init:${STARSTATS_TAG:-latest}
    profiles: [starstats]
    networks: [voyager_t2_proxy]
    restart: "no"
    depends_on:
      postgres:
        condition: service_healthy
    secrets:
      - postgres_default
      - starstats_db_password
      - spicedb_db_password
      - glitchtip_db_password

  # --- StarStats API (Rust + Axum) --------------------------------
  starstats-api:
    image: ${REGISTRY}/starstats/api:${STARSTATS_TAG:-latest}
    profiles: [starstats]
    networks: [voyager_t2_proxy]
    restart: unless-stopped
    depends_on:
      starstats-db-init:
        condition: service_completed_successfully
    environment:
      STARSTATS_BIND: 0.0.0.0:8080
      STARSTATS_DB_HOST: postgres
      STARSTATS_DB_PORT: "5432"
      STARSTATS_DB_USER: starstats_app
      STARSTATS_DB_NAME: starstats
      STARSTATS_DB_PASSWORD_FILE: /run/secrets/starstats_db_password
      STARSTATS_JWT_ISSUER: https://app.example.com
      STARSTATS_JWT_AUDIENCE: starstats
      STARSTATS_JWT_KEY_FILE: /var/lib/starstats/jwt-key.pem
      # Tauri auto-update manifest. The CI release pipeline emits
      # `updater-manifest.json` as a GitHub release asset; pull it
      # onto the host beside the JWT key. Endpoint stays 204 until
      # the file exists, so deploys are safe to do in any order.
      STARSTATS_UPDATER_MANIFEST_PATH: /var/lib/starstats/updater-manifest.json
      OTEL_EXPORTER_OTLP_ENDPOINT: http://otel-collector:4317
      OTEL_SERVICE_NAME: starstats-api
      RUST_LOG: info,starstats=debug,sqlx=warn
      # SMTP is optional. When absent, the server boots normally and
      # signups still succeed — verification emails simply aren't sent
      # (the server logs each would-be send). Set SMTP_URL to enable
      # the Lettre transport; the other knobs are convenience overrides.
      SMTP_URL: smtps://username:password@smtp.example.com:465  # optional
      SMTP_FROM_ADDR: noreply@app.example.com
      SMTP_FROM_NAME: StarStats
      SMTP_WEB_ORIGIN: https://app.example.com
    secrets:
      - starstats_db_password
    volumes:
      - starstats_api_state:/var/lib/starstats
    labels:
      - traefik.enable=true
      - traefik.docker.network=voyager_t2_proxy
      - traefik.http.routers.starstats-api.rule=Host(`api.example.com`)
      - traefik.http.routers.starstats-api.entrypoints=websecure
      - traefik.http.routers.starstats-api.tls.certresolver=letsencrypt
      - traefik.http.services.starstats-api.loadbalancer.server.port=8080

  # --- StarStats web (Next.js) ------------------------------------
  starstats-web:
    image: ${REGISTRY}/starstats/web:${STARSTATS_TAG:-latest}
    profiles: [starstats]
    networks: [voyager_t2_proxy]
    restart: unless-stopped
    depends_on:
      starstats-api:
        condition: service_started
    environment:
      NODE_ENV: production
      PORT: "3000"
      HOSTNAME: 0.0.0.0
      STARSTATS_API_URL: http://starstats-api:8080
      # OTel: gRPC exporter on the same collector port the API uses.
      OTEL_EXPORTER_OTLP_ENDPOINT: http://otel-collector:4317
      OTEL_EXPORTER_OTLP_PROTOCOL: grpc
      OTEL_SERVICE_NAME: starstats-web
      # Sentry-protocol error reports → GlitchTip. Optional. Empty/unset
      # disables shipping; the user-facing error.tsx still renders.
      # Populate after first GlitchTip bring-up creates the project DSN.
      # GLITCHTIP_DSN: https://<project-key>@errors.example.com/<project-id>
    labels:
      - traefik.enable=true
      - traefik.docker.network=voyager_t2_proxy
      - traefik.http.routers.starstats-web.rule=Host(`app.example.com`)
      - traefik.http.routers.starstats-web.entrypoints=websecure
      - traefik.http.routers.starstats-web.tls.certresolver=letsencrypt
      - traefik.http.services.starstats-web.loadbalancer.server.port=3000

  # --- SpiceDB (authorization) ------------------------------------
  starstats-spicedb:
    image: authzed/spicedb:latest
    profiles: [starstats]
    networks: [voyager_t2_proxy]
    restart: unless-stopped
    command: ["serve", "--grpc-preshared-key-file", "/run/secrets/spicedb_preshared_key"]
    environment:
      SPICEDB_DATASTORE_ENGINE: postgres
      SPICEDB_DATASTORE_CONN_URI_FILE: /run/secrets/spicedb_db_password # actually used by entrypoint
      # The image's entrypoint composes the URL itself; if your version
      # requires the full DSN, switch to:
      # SPICEDB_DATASTORE_CONN_URI: postgres://spicedb_app:${SPICEDB_DB_PASSWORD}@postgres:5432/spicedb
    depends_on:
      starstats-spicedb-migrate:
        condition: service_completed_successfully
    secrets:
      - spicedb_preshared_key
      - spicedb_db_password

  starstats-spicedb-migrate:
    image: authzed/spicedb:latest
    profiles: [starstats]
    networks: [voyager_t2_proxy]
    restart: "no"
    command: ["migrate", "head"]
    environment:
      SPICEDB_DATASTORE_ENGINE: postgres
    depends_on:
      starstats-db-init:
        condition: service_completed_successfully
    secrets:
      - spicedb_db_password

  # --- MinIO (object storage for audit/archive) -------------------
  starstats-minio:
    image: minio/minio:latest
    profiles: [starstats]
    networks: [voyager_t2_proxy]
    restart: unless-stopped
    command: ["server", "/data", "--console-address", ":9001"]
    environment:
      MINIO_ROOT_USER: starstats
      MINIO_ROOT_PASSWORD_FILE: /run/secrets/starstats_minio_root_password
    secrets:
      - starstats_minio_root_password
    volumes:
      - starstats_minio_data:/data
    labels:
      - traefik.enable=true
      - traefik.docker.network=voyager_t2_proxy
      # S3 API
      - traefik.http.routers.starstats-s3.rule=Host(`s3.example.com`)
      - traefik.http.routers.starstats-s3.entrypoints=websecure
      - traefik.http.routers.starstats-s3.tls.certresolver=letsencrypt
      - traefik.http.routers.starstats-s3.service=starstats-s3
      - traefik.http.services.starstats-s3.loadbalancer.server.port=9000
      # Console UI
      - traefik.http.routers.starstats-minio.rule=Host(`minio.example.com`)
      - traefik.http.routers.starstats-minio.entrypoints=websecure
      - traefik.http.routers.starstats-minio.tls.certresolver=letsencrypt
      - traefik.http.routers.starstats-minio.service=starstats-minio
      - traefik.http.services.starstats-minio.loadbalancer.server.port=9001

  # --- GlitchTip (Sentry-compatible error tracking) ---------------
  glitchtip-web:
    image: glitchtip/glitchtip:latest
    profiles: [starstats]
    networks: [voyager_t2_proxy]
    restart: unless-stopped
    environment:
      DATABASE_URL: postgres://glitchtip_app@postgres:5432/glitchtip
      SECRET_KEY_FILE: /run/secrets/glitchtip_secret_key
      DEFAULT_FROM_EMAIL: noreply@example.com
      REDIS_URL: redis://redis:6379/3
    depends_on:
      starstats-db-init:
        condition: service_completed_successfully
    secrets:
      - glitchtip_db_password
      - glitchtip_secret_key
    labels:
      - traefik.enable=true
      - traefik.docker.network=voyager_t2_proxy
      - traefik.http.routers.glitchtip.rule=Host(`errors.example.com`)
      - traefik.http.routers.glitchtip.entrypoints=websecure
      - traefik.http.routers.glitchtip.tls.certresolver=letsencrypt
      - traefik.http.services.glitchtip.loadbalancer.server.port=8000

  glitchtip-worker:
    image: glitchtip/glitchtip:latest
    profiles: [starstats]
    networks: [voyager_t2_proxy]
    restart: unless-stopped
    command: ["./bin/run-celery-with-beat.sh"]
    environment:
      DATABASE_URL: postgres://glitchtip_app@postgres:5432/glitchtip
      SECRET_KEY_FILE: /run/secrets/glitchtip_secret_key
      REDIS_URL: redis://redis:6379/3
    depends_on:
      glitchtip-web:
        condition: service_started
    secrets:
      - glitchtip_db_password
      - glitchtip_secret_key

  # --- Observability (separate profile, optional) -----------------
  loki:
    image: ${REGISTRY}/starstats/loki:${STARSTATS_TAG:-latest}
    profiles: [observability]
    networks: [voyager_t2_proxy]
    restart: unless-stopped
    command: ["-config.file=/etc/loki/config.yaml"]
    volumes:
      - loki_data:/loki

  tempo:
    image: ${REGISTRY}/starstats/tempo:${STARSTATS_TAG:-latest}
    profiles: [observability]
    networks: [voyager_t2_proxy]
    restart: unless-stopped
    command: ["-config.file=/etc/tempo/config.yaml"]
    volumes:
      - tempo_data:/var/tempo

  prometheus:
    image: ${REGISTRY}/starstats/prometheus:${STARSTATS_TAG:-latest}
    profiles: [observability]
    networks: [voyager_t2_proxy]
    restart: unless-stopped
    command:
      - --config.file=/etc/prometheus/prometheus.yml
      - --storage.tsdb.path=/prometheus
    volumes:
      - prometheus_data:/prometheus

  otel-collector:
    image: ${REGISTRY}/starstats/otel-collector:${STARSTATS_TAG:-latest}
    profiles: [observability]
    networks: [voyager_t2_proxy]
    restart: unless-stopped
    command: ["--config=/etc/otel-collector/config.yaml"]
    depends_on:
      - loki
      - tempo
      - prometheus
```

After merging this block into `voyager-compose.yml`, validate the
file before bring-up:

```bash
docker compose -f voyager-compose.yml --profile starstats config | head -40
```

If `config` parses without errors, proceed to bring-up.

## Bring-up sequence

```bash
# Activate the starstats profile — runs db-init + spicedb-migrate as oneshots,
# then starts spicedb, minio, api, web, glitchtip-web, glitchtip-worker
docker compose -f voyager-compose.yml --profile starstats up -d

# Activate the observability stack (independent, can run before or after)
docker compose -f voyager-compose.yml --profile observability up -d

# Verify
docker compose ps --profile starstats
docker compose logs starstats-api --tail=50
docker compose logs starstats-web --tail=50
```

After first bring-up, apply the SpiceDB schema:

```bash
docker run --rm --network voyager_t2_proxy \
  -v ${DOCKERDIR}/starstats/spicedb:/schema:ro \
  authzed/zed:latest \
  --endpoint spicedb:50051 \
  --token "$(cat ${SECRETSDIR}/spicedb_preshared_key)" \
  --insecure \
  schema write /schema/schema.zed
```

## Connect Grafana datasources

In existing Grafana (`grafana.example.com`) → Connections → Data sources:

| Name | Type | URL |
|---|---|---|
| Loki | Loki | `http://loki:3100` |
| Tempo | Tempo | `http://tempo:3200` |
| Prometheus | Prometheus | `http://prometheus:9090` |

Tempo → Trace to logs: link to Loki by `service_name` and `trace_id`.
Loki → Derived field: `traceID=(\w+)` → link to Tempo.

## Cloudflare DNS

The existing `cloudflare-companion` service auto-mirrors Traefik hosts.
After `docker compose up`, the following A records appear under `example.com`:

- `stats`
- `stats-api`
- `stats-s3`
- `stats-minio`
- `stats-errors`

No manual DNS work required.

## Verifying end-to-end

```bash
# API health
curl https://api.example.com/healthz

# Web health
curl https://app.example.com/api/healthz

# JWKS (Slice 5 — uncomment when /.well-known/jwks.json lands)
# curl https://app.example.com/.well-known/jwks.json

# MinIO buckets (via mc)
docker run --rm --network voyager_t2_proxy minio/mc \
  alias set local http://starstats-minio:9000 starstats "$(cat ${SECRETSDIR}/starstats_minio_root_password)"

# SpiceDB schema check
docker run --rm --network voyager_t2_proxy authzed/zed:latest \
  --endpoint spicedb:50051 \
  --token "$(cat ${SECRETSDIR}/spicedb_preshared_key)" \
  --insecure schema read
```

## Bucket setup (once)

After MinIO comes up, create the two required buckets and enable
Object Lock on the audit bucket:

```bash
docker run --rm --network voyager_t2_proxy minio/mc sh -c "
  mc alias set local http://starstats-minio:9000 starstats $(cat ${SECRETSDIR}/starstats_minio_root_password) && \
  mc mb --with-lock local/starstats-audit && \
  mc mb local/starstats-archive && \
  mc retention set --default compliance 7y local/starstats-audit
"
```

## Backups

Pragmatic backup commands for the bits that actually hurt to lose.
Drop these into the existing voyager backup cron — the host already
has a `/backups` mount you can target.

### Postgres (StarStats + SpiceDB + GlitchTip)

All three live in the shared `postgres` container; back them up
individually so a partial restore is possible:

```bash
# /etc/cron.daily/starstats-pg-dump
docker exec postgres pg_dump -U postgres starstats | gzip > /backups/starstats-$(date +%F).sql.gz
docker exec postgres pg_dump -U postgres spicedb   | gzip > /backups/spicedb-$(date +%F).sql.gz
docker exec postgres pg_dump -U postgres glitchtip | gzip > /backups/glitchtip-$(date +%F).sql.gz

# Retain ~30 days of dailies; weekly/monthly handled by your existing rotation.
find /backups -name 'starstats-*.sql.gz' -mtime +30 -delete
find /backups -name 'spicedb-*.sql.gz'   -mtime +30 -delete
find /backups -name 'glitchtip-*.sql.gz' -mtime +30 -delete
```

### JWT signing key (irreplaceable)

`${DOCKERDIR}/starstats/api-state/jwt-key.pem` is the RSA private key
that mints every device + user JWT. **Losing it invalidates every
token in flight** — every desktop client and every web session has
to re-pair / re-login until the new keypair is generated. Treat it
like a CA root.

```bash
# /etc/cron.daily/starstats-jwt-backup
install -m 0600 ${DOCKERDIR}/starstats/api-state/jwt-key.pem \
                /backups/jwt-key-$(date +%F).pem
```

Then mirror `/backups/jwt-key-*.pem` offsite (a second host, a cold
S3 bucket with versioning, or an encrypted USB cycled weekly — pick
one that survives the voyager box being lost). Do **not** put the
key in a backup that other tenants of the host can read.

### MinIO buckets

`starstats-audit` has Object Lock in compliance mode (7-year retention
applied at bucket creation — see "Bucket setup" above). The Object
Lock guarantee covers ransomware/tamper, but **not** "the host
filesystem is gone." Two options:

```bash
# Option A: replicate to a second MinIO via mc mirror, weekly is fine
# (the audit bucket is append-only; full-tree mirror is cheap).
docker run --rm --network voyager_t2_proxy minio/mc sh -c "
  mc alias set local  http://starstats-minio:9000 starstats $(cat ${SECRETSDIR}/starstats_minio_root_password) && \
  mc alias set offsite https://minio-offsite.example.com offsite-user $(cat ${SECRETSDIR}/offsite_minio_password) && \
  mc mirror --preserve --remove local/starstats-audit offsite/starstats-audit && \
  mc mirror --preserve --remove local/starstats-archive offsite/starstats-archive
"

# Option B: rely on the host filesystem backup of the
# `starstats_minio_data` volume (works if your voyager host backup
# already snapshots `/var/lib/docker/volumes/`).
```

`starstats-archive` has no Object Lock — back it up the same way.

### Restore drill (quarterly)

Test restores quarterly: pick a recent `starstats-YYYY-MM-DD.sql.gz`,
restore it into a throwaway Postgres container, and confirm a
representative query returns expected row counts. Same shape for the
JWT key (verify it loads via `openssl rsa -in jwt-key-*.pem -check`)
and MinIO (`mc ls offsite/starstats-audit | head`). Untested backups
are not backups — schedule the drill in the voyager runbook so it
gets executed, not just intended.

## Tearing down

```bash
# Stop stack but keep data:
docker compose --profile starstats --profile observability down

# Wipe all StarStats state (DESTRUCTIVE):
docker compose --profile starstats --profile observability down -v
docker volume rm voyager_starstats_minio_data voyager_loki_data voyager_tempo_data voyager_prometheus_data
# DBs in shared postgres are NOT removed — drop manually:
docker exec -it postgres psql -U postgres -c "DROP DATABASE starstats; DROP DATABASE spicedb; DROP DATABASE glitchtip;"
```

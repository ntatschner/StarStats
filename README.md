# StarStats

EAC-safe personal metrics platform for Star Citizen. Tray client + API + web app.
**Reads only files the game writes to disk and the RSI website with your own session** — never the running game process.

## Repository layout

```
StarStats/
├── Cargo.toml                  Rust workspace
├── pnpm-workspace.yaml         pnpm + Turborepo
├── crates/
│   ├── starstats-core/         shared types, parser, validators
│   ├── starstats-server/       API server (Axum + sqlx)
│   └── starstats-client/       Tauri tray app (Rust backend)
├── apps/
│   ├── tray-ui/                Tauri webview frontend (Vite + React)
│   └── web/                    Next.js dashboard
├── packages/
│   └── api-client-ts/          generated TS client (from server's OpenAPI spec)
├── infra/                      docker-compose configs (synced to homelab host)
├── docs/
│   ├── ARCHITECTURE.md         (forthcoming — replaces python-spike doc)
│   ├── HOMELAB-INTEGRATION.md  bring-up runbook for voyager docker host
│   ├── OBSERVABILITY.md        per-component telemetry matrix
│   └── AUDIT.md                tamper-evident audit log design
└── prototypes/
    └── python-spike/           original Python prototype, retained for reference
```

## Stack at a glance

- **Tray client**: Rust + Tauri 2 + React (TS) — single signed binary on Windows + Linux
- **Server**: Rust + Axum + sqlx + Postgres (existing voyager pgvector/pg17)
- **Web**: Next.js 15 (App Router, RSC) + TypeScript + Tailwind v4 + shadcn/ui
- **Authn**: Self-hosted (RS256 JWTs minted by the API; web app holds email/pw + OAuth link UI)
- **Authz**: SpiceDB (Zanzibar-style ReBAC) for org/sharing/scopes
- **Object storage**: MinIO with Object Lock for audit immutability
- **Observability**: OpenTelemetry → Loki / Tempo / Prometheus, surfaced in existing Grafana
- **Errors**: GlitchTip (Sentry-compatible)

See `docs/HOMELAB-INTEGRATION.md` for the full bring-up runbook on the
voyager docker host.

## Development

Local-dev environment variables are documented in two `.env.example`
files — copy each to its sibling and tweak as needed:

```powershell
Copy-Item crates/starstats-server/.env.example crates/starstats-server/.env
Copy-Item apps/web/.env.example apps/web/.env.local
```

Then:

```powershell
# Rust — workspace build, test, lint
cargo build --workspace --exclude starstats-client
cargo test  --workspace --exclude starstats-client
cargo clippy --workspace --exclude starstats-client -- -D warnings
cargo fmt --all --check

# Tauri client (requires platform deps — see crates/starstats-client/README.md)
cargo build -p starstats-client

# Web app
pnpm install
pnpm --filter web build
pnpm --filter web dev

# Tray UI (Vite frontend for Tauri)
pnpm --filter tray-ui dev
```

`starstats-client` is excluded from default workspace builds because Tauri
requires GTK / WebKit2GTK on Linux and WebView2 on Windows. CI builds it on
matrix runners with the proper system deps.

## Deployment

Deployment to a self-hosted compose stack is documented end-to-end in
[`docs/HOMELAB-INTEGRATION.md`](docs/HOMELAB-INTEGRATION.md), including
the exact `services:` block to merge into your compose file, secret
provisioning, JWT-key bootstrap, and bring-up sequence. CI publishes
container images on tag push (`v*`) via
[`.github/workflows/release.yml`](.github/workflows/release.yml).

## What StarStats does NOT do

- Read game memory
- Inject into the game process
- Hook game APIs
- Sniff or modify game network traffic
- Modify any game files

If it can be detected by EAC, it isn't in this tool.

## Licence

Licensed under the Mozilla Public License, v. 2.0. See
[`LICENSE`](LICENSE) for the full text. MPL-2.0 is file-level
copyleft: modifications to existing source files must be released
under the same terms, but the code can be combined with proprietary
or other-licensed code in larger works.

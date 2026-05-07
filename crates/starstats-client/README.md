# starstats-client

Tauri 2 tray client for StarStats. Excluded from the default workspace
build because Tauri requires platform-specific system dependencies.

## Building

### Linux (Debian/Ubuntu)

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev pkg-config
cargo build -p starstats-client
```

### Windows

WebView2 ships with Windows 10+ so no extra system deps. Just:

```powershell
cargo build -p starstats-client
```

## Dev loop

From the repo root:

```bash
pnpm install                  # install tray-ui deps once
pnpm tauri:dev                # vite dev server + tauri rebuild on save
```

## Icons

Drop platform icons into `icons/` (32x32.png, 128x128.png, icon.png /
icon.ico / icon.icns). Tauri picks the right one per bundle target.
Placeholder icons land in Phase 1.

# Handoff: StarStats — Local-first Star Citizen telemetry app

## Overview
StarStats is a local-first telemetry app for Star Citizen players. A desktop client parses the game's log file on the user's PC; structured events (logins, deaths, missions, jumps, etc.) sync to a web app where the user gets a manifest, dashboards, public profile, multi-device pairing, and crowd-sourced log-pattern discovery.

This handoff bundles the full hi-fi web prototype (everything **except** the desktop client itself).

## About the Design Files
The files under `prototype/` are **design references created in HTML/JSX** running on in-browser Babel + React 18. They are *not* production code to copy directly.

The task is to **recreate these designs in the target codebase's environment** (whatever framework the project uses — Next.js, Remix, Vite + React, etc.), using its established patterns, routing, data layer, and component primitives.

If no environment exists yet, **Next.js (App Router) + Tailwind + shadcn/ui** is a good fit for this design — the structure already maps cleanly to file-based routes and the visual language is compatible with shadcn primitives.

## Fidelity
**High-fidelity.** All colors, spacing, typography, and interactions are final. Recreate pixel-perfectly. Use exact tokens from `prototype/tokens.css`.

## How to read the prototype
1. Open `prototype/StarStats Prototype.html` in a browser.
2. Use the **Tweaks** panel (toolbar toggle) to switch theme, type pairing, accent intensity, and jump between any of 14 screens.
3. Open `prototype/StarStats Responsive Canvas.html` to see Mobile (390) / Tablet (820) / Desktop (1440) side-by-side. The picker syncs all three.
4. Read `DESIGN_REFERENCE.md` (top level) — it's a structural map: every screen, file, component, expected data shape, and the **Backend gaps** section calling out what's net-new vs. existing backend.

## Screens / Views
14 screens. Detail in `DESIGN_REFERENCE.md` — each with file, components, and expected data shape. Summary:

**Marketing**
- Landing — hero with rotating word swap, 6 feature cards, mock dashboard preview, footer

**Auth**
- Login, Magic-link sent, TOTP verify, Sign up

**Main app (left rail nav)**
- Dashboard — heatmap (26-week × 7-day), stat strip, top types, recent activity
- Hangar (Devices) — pair-code flow, paired clients list with revoke
- Orgs — RSI org links with visibility controls
- Public profile — public-safe view with verified RSI handle, supporter pill, theme accent

**Data section** (new — see Backend gaps)
- Metrics — 4 tabs: Overview, Event types, Sessions, Raw stream
- My logs — 4 tabs: Overview, Batches, Health, Storage. Includes a `BatchDetail` drilldown
- Submissions — crowd-sourced log-pattern discovery. List + detail with vote, flag, lifecycle

**Account / system**
- Settings, Two-factor wizard (4-step), Donate (One-off £5 + Monthly £1), Download client (Win/Mac/Linux), Components sheet (dev-only)

## Interactions & Behavior
- **Page transitions**: 460ms fade + 14px lift + blur unwind on every screen change
- **Card stagger**: 60ms entrance stagger when a screen mounts
- **Hover lift**: cards rise with accent-tinted border + soft shadow
- **Hero rotator**: word in hero swaps with vertical drift + letter-spacing tighten + accent gradient sweep
- **Background motion (`quantum-warp.jsx`)**: 90 parallax streaks moving left-ish; per-screen direction (e.g., signup=up, download=down, donate=up-right). Smooth tween between directions on screen change. **Always recreate this if possible — it's the signature.**
- **Tweaks panel**: live theme/type/accent switching; jump-to-screen dropdown
- **Responsive**: at ≤1024px the rail collapses to icon-only; at ≤640px to a hamburger drawer
- **Reduced motion**: all animations respect `prefers-reduced-motion`

## State Management
Per-screen local state for filters/tabs/forms. Global state needed:
- Auth session
- Current user (handle, supporter status, theme preference, name plate, retention tier)
- Online clients count (live from server)
- Theme + tweaks (persistable per-user)

Server data via standard fetcher (TanStack Query / SWR / RTK Query — pick what fits the codebase).

## Design Tokens
**Source of truth: `prototype/tokens.css`.** Copy values verbatim — don't approximate.

**Themes** — switched via `[data-theme="..."]` on root:
- `stanton` (default): warm amber on charcoal — `--bg: #0F0E12`, `--accent: #E8A23C`, `--fg: #ECE7DD`
- `pyro`: molten coral — `--bg: #100C0E`, `--accent: #F25C3F`
- `terra`: cool teal — `--bg: #0B1014`, `--accent` is teal
- `nyx`: light bg, deep violet

**Type**: Geist + Geist Mono (default). Inter and IBM Plex selectable.
**Spacing**: 4 / 8 / 12 / 16 / 24 / 32 / 48 / 64
**Radii**: 4 / 6 / 10 / 14 / 20 / pill
**Motion**: ease-out `cubic-bezier(0.22, 1, 0.36, 1)`, fast 120ms / base 180ms / slow 320ms
**Type scale**: 11 / 13 / 14 / 16 / 20 / 28 / 40 / 64 px
**Status colors**: ok `#74C68A`, warn `#E8C53C`, danger `#E8674C`, info `#6FA8E8`

## Voice / copy
In-universe Star Citizen voice for chrome. Errors stay dry.

| Term | Means |
|------|-------|
| Comm-Link | Email |
| Hangar | Paired devices |
| Manifest | Event archive / dashboard |
| Authentication code | TOTP code |
| "Scope is clear." | Empty state |

## Assets
- **Icons**: stroke set in `prototype/icons.jsx` — recreate as Lucide/Tabler equivalents in target codebase
- **Fonts**: Geist + Geist Mono (Google Fonts) — alternatives Inter or IBM Plex
- **Imagery**: none. Hero is type + heatmap mock. No real photos used.
- **Brand mark**: `★` glyph + STARSTATS wordmark (rendered in CSS)

## Backend gaps (read this carefully)
Several pages assume backend capabilities that may not exist yet. See **DESIGN_REFERENCE.md → Backend gaps summary** for the full list. Headlines:

1. **Submissions system** (whole new domain) — tables for submissions/votes/flags/comments + endpoints + mod tools + parser-release pipeline
2. **Batch-level introspection** — keep raw batches addressable by ID with line-by-line drill-down, retry, delete, hash
3. **Health telemetry** — per-client success rates, sequence-gap detection, clock-drift, recent-incidents feed
4. **Storage quota & per-category accounting** — byte tracking + free-tier caps
5. **Donate / supporter mechanics** — Stripe webhook, name plate (28-char cap) persistence, retention extension, partial revert on cancel
6. **Handle verification** — RSI bio scraping with one-time code (1-min window)
7. **Org membership re-verification** weekly job
8. **2FA recovery codes** — single-use tracking
9. **Async export jobs** — NDJSON / zip / CSV with emailed download links

Each component renders directly from the data constants at the top of its `.jsx` file (e.g., `LOG_BATCHES`, `SUBMISSIONS`, `METRICS_EVENT_TYPES`). **Match those field names and shapes exactly when designing API responses** — saves a lot of mapping in the frontend.

## Files in this bundle
```
design_handoff_starstats/
├── README.md                       (this file)
├── DESIGN_REFERENCE.md             ← structural map: pages, components, data shapes, backend gaps
└── prototype/
    ├── StarStats Prototype.html    ← entry point
    ├── StarStats Responsive Canvas.html
    ├── tokens.css                  ← all design tokens + responsive CSS
    ├── app.jsx                     ← routing + theme + Tweaks
    ├── icons.jsx
    ├── primitives.jsx              ← Card, Button, Badge, KV, Stack, Row, etc.
    ├── tweaks-panel.jsx
    ├── shell-and-landing.jsx       ← TopBar, LeftRail, NAV, Landing
    ├── auth-screens.jsx
    ├── app-screens.jsx             ← Dashboard, Devices, Orgs, Profile
    ├── settings-and-system.jsx     ← Settings, 2FA, Components, Download
    ├── donate.jsx
    ├── data-screens.jsx            ← Metrics, Submissions list + detail
    ├── my-logs.jsx                 ← My logs + BatchDetail
    ├── quantum-warp.jsx            ← signature background animation
    └── design-canvas.jsx
```

## Recommended implementation order
1. Tokens + theme provider (`tokens.css` → CSS vars or Tailwind theme)
2. Primitives (Card, Button, Badge, KV, Stack, Row)
3. App shell (TopBar + LeftRail + drawer + responsive breakpoints)
4. Dashboard (anchors most patterns: heatmap, stat strip, ranked bars, timeline)
5. Auth flow
6. Devices, Orgs, Profile, Settings
7. **Data section** — Metrics, My logs, Submissions (highest backend coordination)
8. Donate, Download
9. Quantum-warp background (last — visual polish)
10. Animations + reduced-motion

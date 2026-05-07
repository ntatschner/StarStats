# StarStats — Design Reference (Supplementary)

> **This document is supplementary to the primary design doc.**
> It does not replace product/UX intent. It is a structural map of the
> built prototype: which pages exist, which file/component renders each,
> what data shape they expect, and where the design has diverged from
> the current backend.
>
> Read the primary design doc for *why*. Read this for *what* and *where*.

A complete map of what's been designed in `StarStats Prototype.html` — every page, every component, what it does, what file it lives in, and what the backend needs to support it.

Use the **Backend gaps** notes to plan implementation work against the existing backend.

---

## Project structure

| File | Purpose |
|------|---------|
| `StarStats Prototype.html` | Entry point. Loads React, Babel, fonts, and all `.jsx` modules in dependency order. |
| `StarStats Responsive Canvas.html` | Side-by-side preview of the prototype at Mobile (390), Tablet (820), Desktop (1440). Picker syncs all three frames. |
| `tokens.css` | Design tokens — color, type, spacing, themes (Stanton/Pyro/Terra/Nyx), responsive breakpoints, all `.ss-*` component classes. |
| `tweaks-panel.jsx` | Reusable Tweaks framework (used here for theme/type/accent switching + jump-to-screen). |
| `icons.jsx` | Icon set (`I.chart`, `I.signal`, etc). Stroke icons + GitHub mark. |
| `primitives.jsx` | `Card`, `Button`, `Badge`, `Pill`, `KV`, `Field`, `Stack`, `Row`, table styles. |
| `shell-and-landing.jsx` | App shell (`TopBar`, `LeftRail`, `DrawerScrim`, NAV map) + landing page. |
| `auth-screens.jsx` | Login, magic-link confirm, TOTP verify, signup. |
| `app-screens.jsx` | Dashboard, Devices (Hangar), Orgs, Public profile. |
| `settings-and-system.jsx` | Settings, 2FA wizard, Components sheet, Download client. |
| `donate.jsx` | Donate / supporter tiers page. |
| `data-screens.jsx` | Metrics, Submissions list, Submission detail. |
| `my-logs.jsx` | My logs archive with batch detail view. |
| `quantum-warp.jsx` | Animated streaking-stars background canvas. Per-screen direction. |
| `app.jsx` | Top-level `<StarStatsApp>`: routes between screens, hosts theme + Tweaks state. |

---

## Navigation

Defined in `shell-and-landing.jsx` → `NAV` array. Left rail is grouped:

- **main**: Dashboard, Hangar, Orgs, Public profile
- **data**: Metrics, My logs, Submissions
- **account**: Settings, Two-factor, Donate
- **system**: Components

Top bar shows: hamburger (mobile), brand mark / current screen, online client pill + avatar.

---

## Pages

### 1. Landing — `shell-and-landing.jsx` → `Landing`
Marketing page. No app shell.

**Components:**
- Hero with rotating word swap (brand name ↔ feature callouts: "Your manifest." / "Your numbers." / "Your timeline.")
- 6 feature cards (local-first, structured events, share controls, multi-device, manifest export, hardened auth)
- Mock dashboard preview (heatmap + numbers)
- Top nav: Features / Privacy / Download / GitHub / Sign in / Get started
- Footer: Privacy / Terms / GitHub repo link

**Backend needs:** None. Pure marketing.

---

### 2. Auth screens — `auth-screens.jsx`

#### `Login`
- Comm-Link (email) + password fields
- "Send magic link instead" button
- "Create an account" link → `signup`
- "Sign in" → POSTs creds → if 2FA enabled → `totp` screen

#### `MagicSent`
- Confirmation screen after magic-link request
- "Open Comm-Link" + "Resend" actions

#### `TotpVerify`
- 6-digit code input + "Use a recovery code instead" link

#### `Signup`
- Comm-Link + password + confirm
- T&C checkbox
- Submit → creates account → land on dashboard

**Backend needs:**
- `POST /auth/login` (email, password) → session OR `requires_2fa: true`
- `POST /auth/magic-link` (email) → emails token
- `GET /auth/magic-link/:token` → consumes, returns session
- `POST /auth/2fa/verify` (code) → session
- `POST /auth/signup` (email, password) → creates user, sends verification email
- `POST /auth/2fa/recovery` (code) → session, marks code used

---

### 3. Dashboard — `app-screens.jsx` → `Dashboard`

**Components:**
- Greeting header w/ handle, last sync timestamp
- Stat strip: Total events / Hours played / Sessions / Top system
- Heatmap (26-week × 7-day grid, intensity by event count)
- Top event types ranked bar chart
- Recent activity timeline (10 events, with type tags)

**Backend needs:**
- `GET /me/dashboard` returning:
  - `total_events`, `hours_played`, `session_count`, `top_system`, weekly deltas
  - `heatmap`: `[{date, count}, ...]` for last 26 weeks
  - `top_event_types`: `[{type, count}, ...]` top N
  - `recent_events`: latest 10–20 normalized events

---

### 4. Hangar (Devices) — `app-screens.jsx` → `Devices`

**Components:**
- Pairing code card (4-block code, QR placeholder, expiry timer)
- Paired devices list (name, last seen, OS, batches sent, status)
- Per-device actions: rename, revoke, view logs

**Backend needs:**
- `POST /devices/pair-code` → returns one-time 8-char code, expires in 10min
- `POST /devices/pair` (code, device fingerprint) → device record + per-device API token
- `GET /devices` → paired list with `last_seen`, `batches_count`, `status`
- `PATCH /devices/:id` → rename
- `DELETE /devices/:id` → revoke (invalidates token, stops accepting batches)

---

### 5. Orgs — `app-screens.jsx` → `Orgs`

**Components:**
- Linked orgs list (RSI org SID, member role, member count)
- Add-org flow placeholder
- Per-org settings: visibility, data sharing scope

**Backend needs:**
- `POST /orgs/link` (RSI org SID) → verifies user is a member via RSI scrape, creates link
- `GET /me/orgs` → linked orgs
- `PATCH /me/orgs/:sid` → visibility, share scope
- `DELETE /me/orgs/:sid` → unlink
- Background job: re-verify membership weekly; auto-unlink if removed

---

### 6. Public profile — `app-screens.jsx` → `PublicProfile`

**Components:**
- Profile header: avatar, RSI handle (verified badge), bio
- Supporter pill (if donated) + custom name plate (if monthly tier)
- Stat tiles, heatmap, top types — public versions
- Visibility selector preview (public / handle-only / org-only / private)

**Backend needs:**
- `GET /u/:handle` → public-safe view (respects user's visibility setting)
- `PUT /me/profile` → bio, visibility, theme accent (supporter only)
- Handle verification flow:
  - `POST /me/handle/start-verify` → returns 1-min one-time code
  - Background poll of RSI bio → on detect, mark verified, code burns
  - `POST /me/handle/cancel-verify`

---

### 7. Settings — `settings-and-system.jsx` → `Settings`

**Components:**
- Account info (email, password change, name plate if supporter, retention policy)
- Theme switcher (4 themes — wires to Tweaks panel)
- Notification preferences
- Danger zone (export all, delete account)

**Backend needs:**
- `PATCH /me/account` (email, password)
- `POST /me/export` → kicks off async export job → emails download link
- `DELETE /me/account` → 30-day grace, then full purge
- `GET /me/preferences` / `PUT /me/preferences` (theme, notification toggles)

---

### 8. Two-factor wizard — `settings-and-system.jsx` → `TwoFAWizard`

**Components:**
- Step 1: explainer + start
- Step 2: QR + manual secret (with reveal toggle + copy button — masked dots by default)
- Step 3: Verify with 6-digit code
- Step 4: Save 10 recovery codes (download / copy / print)

**Backend needs:**
- `POST /me/2fa/start` → returns secret (base32) + provisioning URI
- `POST /me/2fa/verify` (code) → enables 2FA, returns 10 single-use recovery codes
- `POST /me/2fa/disable` (current 2FA code) → disables
- Recovery code use is single-shot; tracked server-side

---

### 9. Donate — `donate.jsx` → `Donate`

**Components:**
- Two tiers: One-off £5, Monthly £1
- Live profile preview (supporter pill, custom name plate, accent color update in real time)
- Tier switcher with feature breakdown
- "Where it goes" honest budget breakdown
- "Where it reverts" cancellation explainer
- Stripe checkout button (placeholder)

**Backend needs:**
- `POST /billing/checkout-session` (tier, optional name plate text) → Stripe session URL
- `POST /billing/webhook` (Stripe) → on payment success: mark supporter status, save name plate (28-char cap), set tier, extend retention
- `POST /billing/cancel` → cancel monthly at period end
- `GET /me/billing` → tier, status, renewal date, name plate
- Cancellation handler: revert retention from 5y → 12mo, drop theme accent, KEEP supporter pill + name plate (per design)

---

### 10. Download client — `settings-and-system.jsx` → `Download`

**Components:**
- Three OS cards: Windows / macOS / Linux with sizes, version, download buttons
- Release notes link
- "Already installed?" deep link to Hangar pairing flow

**Backend needs:**
- Static asset hosting for installers (signed binaries)
- `GET /releases/latest` → version metadata + download URLs
- Auto-update channel for desktop client

---

### 11. Metrics — `data-screens.jsx` → `Metrics`

Tab views: **Overview · Event types · Sessions · Raw stream**

**Components:**
- `MetricsOverview`: 4 stat tiles, top types ranked bars (top 6), recent sessions list (top 5)
- `MetricsTypes`: full event-type table — type, count, % of total, last seen, 30d trend
- `MetricsSessions`: full session table — date, time window, duration, primary ship, client, event count
- `MetricsRaw`: timestamped event lines with type label + payload preview

**Data shapes (frontend constants in file):** `METRICS_EVENT_TYPES`, `METRICS_SESSIONS`, `METRICS_RAW`.

**Backend needs (NEW — likely diverged):**
- `GET /me/metrics/event-types?range=30d` → `[{type, count, pct, last_seen, trend_pct_30d}, ...]` for all distinct types
- `GET /me/metrics/sessions?limit=N&offset=M` → paginated session list with start, end, duration, ship_inferred, client_id, event_count
- `GET /me/metrics/events?limit=N&offset=M&type=&session=&from=&to=` → paginated normalized event stream
- Session inference logic: server-side, group events by client_id with N-minute idle gap to define a session; infer "primary ship" by max-time-in-vehicle

---

### 12. My logs — `my-logs.jsx` → `MyLogs`

Tab views: **Overview · Batches · Health · Storage**, plus `BatchDetail` modal-route.

**Components:**
- `LogsOverview`: 4 stat tiles, recent batches (top 6, clickable to detail), parser breakdown bars (parsed/unknown/malformed), 24h throughput sparkline
- `LogsBatches`: search + client filter + full batches table — `BATCH-####`, ts, client, session, lines, parsed, unknown, size, status
- `LogsHealth`: per-client health (batches sent, success rate, latency, last seen, online state) + recent issues feed (sequence gaps, retries, clock drift)
- `LogsStorage`: usage bar + per-category breakdown (parsed events / raw archive / sessions+meta / unknowns) + retention policy + data control (export NDJSON / zip / CSV, purge old)
- `BatchDetail`: raw line table (line #, ts, level, type, raw payload), batch metrics KV (lines, parsed, unknown, compressed/uncompressed size, latency, hash), per-batch actions (download, view unknowns, retry if failed, delete)

**Data shapes:** `LOG_BATCHES`, `BATCH_DETAIL_SAMPLE` in `my-logs.jsx`.

**Backend needs (NEW — likely diverged):**
- `GET /me/logs/batches?limit=&client=&search=` → batch index with `id`, `received_at`, `client_id`, `session_id`, `line_count`, `parsed_count`, `unknown_count`, `compressed_size`, `status`, optional `error`
- `GET /me/logs/batches/:id` → full batch: header + all parsed lines with line_number, raw_text, parsed_type, log_level
- `GET /me/logs/health` → per-client stats: batches_sent, success_rate, avg_latency_ms, last_seen, online_state. Also recent_incidents: gaps, retries, clock_drift events
- `GET /me/logs/storage` → quota_used_bytes, quota_total_bytes, breakdown by category, retention policy
- `POST /me/logs/batches/:id/retry` → re-attempts ingestion of failed batch
- `DELETE /me/logs/batches/:id` → removes batch + derived events
- `POST /me/logs/purge?older_than=30d` → bulk purge
- `GET /me/logs/export?format=ndjson|zip|csv` → async export job
- **Storage tracking:** server must compute and persist per-user storage usage by category. Free tier: 50MB cap. Monthly supporter: unlimited (or 5y retention based on design).
- **Sequence gap detection:** every batch carries a `sequence_number` per client. Server flags non-contiguous receipts as gaps and triggers a `resync_request` event sent back to the client.
- **Hash:** each batch should have content hash stored for integrity checks (visible on detail page).

---

### 13. Submissions — `data-screens.jsx` → `Submissions` + `SubmissionDetail`

Crowd-sourced **type discovery** — when the desktop client encounters an unknown log line shape, it offers to submit the pattern. Community votes prioritize. Mods accept/reject. Accepted ones ship in the next parser update for everyone.

**Components:**
- `Submissions` list: filter tabs (All / In review / Accepted / Rejected / Mine) + 4 stat tiles + paginated list
- `SubmissionRow`: vote button (with optimistic toggle), ID + status pill, label (proposed `event_type` name), description, submitter, occurrences across network, submitter count, pattern preview, flag button
- `SubmissionDetail`:
  - Header: ID, status, label, description
  - Raw pattern card (with wildcard explanation)
  - Sample matches (4 real lines this caught)
  - Discussion thread (comments, comment votes)
  - Sidebar: vote / flag / withdraw (if mine), submission KV (submitter, when, occurrences, submitters, total votes, status), lifecycle progress (Submitted → Community vote → Mod review → Ships in update)

**Status enum:** `pending` · `review` · `accepted` · `shipped` (with version) · `rejected` (with reason) · `flagged`

**Data shapes:** `SUBMISSIONS` in `data-screens.jsx`.

**Backend needs (NEW — does not exist):**
- `submissions` table: `id`, `pattern`, `proposed_label`, `description`, `submitted_by`, `submitted_at`, `status`, `rejection_reason`, `shipped_in_version`, `occurrences` (denormalized count), `submitter_count`, `vote_count`
- `submission_votes` table: `(submission_id, user_id)` unique
- `submission_flags` table: `(submission_id, user_id, reason)`
- `submission_comments` table: id, submission_id, user_id, body, created_at, vote_count
- `POST /submissions` (pattern, proposed_label, description) — auto-dedupes by pattern; if exists, increments submitter_count + occurrences
- `GET /submissions?filter=all|review|accepted|rejected|mine&page=` → list
- `GET /submissions/:id` → detail incl. comments, sample matches (joined from `events` table via pattern), lifecycle state
- `POST /submissions/:id/vote` / `DELETE /submissions/:id/vote` → toggle vote
- `POST /submissions/:id/flag` (reason)
- `POST /submissions/:id/comment` (body)
- `POST /submissions/:id/comment/:cid/vote`
- `DELETE /submissions/:id` (only by submitter, only if status=pending|review)
- **Mod tools (separate admin UI):** approve/reject, set proposed_label, schedule for next parser release
- **Auto-dedup:** when a desktop client submits a pattern, normalize wildcards and check for existing match before creating a new submission record. Increment counters on existing.
- **3-flag escalation:** when 3 verified handles flag a submission, auto-set status to `flagged` and route to mod queue
- **Vote threshold:** soft signal at 500 votes ("ready for mod review"); not a hard gate
- **Parser update flow:** mod accepts → assigns to next semver release → on release ship, status flips to `shipped` and version is recorded → desktop clients pull new parser config on next sync → re-classify their unknowns historically

---

### 14. Components sheet — `settings-and-system.jsx` → `Components`

Internal dev page — visual showcase of every primitive. Not user-facing.

---

## Theming

`tokens.css` defines four themes via `[data-theme="..."]` on `#ss-root`:

| Theme | Vibe | `--accent` |
|-------|------|------------|
| `stanton` (default) | Warm amber on charcoal | Amber |
| `pyro` | Molten coral | Coral red |
| `terra` | Cool teal | Teal |
| `nyx` | Light bg, deep violet | Violet |

Switchable via Tweaks panel. Type pairing (Geist / Inter / IBM Plex) and accent intensity also live there.

**Backend needs:** Persist user's theme choice in `/me/preferences`.

---

## Voice / copy guidelines

The product uses an in-universe voice for chrome and section names — referencing Star Citizen lore. Errors and forms stay dry.

| Voice term | Means |
|------------|-------|
| Comm-Link | Email |
| Hangar | Paired devices page |
| Manifest | Your event archive / dashboard |
| Authentication code | TOTP / 2FA code |
| "Scope is clear." | Empty state copy |

Apply consistently in any new copy.

---

## Backend gaps summary

The following are **net-new** capabilities the design assumes but the backend (likely) doesn't yet have. Sorted by likely effort:

1. **Storage quota & per-category accounting** (My logs → Storage tab). Need byte tracking by category, enforced caps for free tier.
2. **Submissions system** (whole new domain). Tables + endpoints + mod tools + parser-release pipeline.
3. **Health telemetry** (My logs → Health tab). Per-client success rates, latency tracking, sequence-gap detection, clock-drift detection, recent-incidents feed.
4. **Batch-level introspection** (My logs → Batches + BatchDetail). Need to keep raw batches addressable by ID with line-by-line drill-down, not just the parsed events.
5. **Per-batch retry & delete** endpoints with proper cascade to derived events.
6. **Export jobs** (NDJSON / zip / CSV) — async, emailed link.
7. **Donate / supporter tier mechanics** — Stripe webhook, name plate persistence with 28-char cap, retention extension on tier change, partial revert on cancel.
8. **Handle verification** — RSI bio scraping with one-time code (1-minute window).
9. **Org membership re-verification** weekly job.
10. **Recovery code single-use tracking** for 2FA.

---

## How to read this when building backend

For each page above:
1. Open the referenced `.jsx` file.
2. Find the data constants at the top (e.g. `METRICS_EVENT_TYPES`, `LOG_BATCHES`, `SUBMISSIONS`) — these are the exact response shapes the UI expects.
3. The component renders those shapes directly — match field names and types when designing the API.
4. Anywhere the design shows a stat tile, ranked bar, or trend, the backend needs to compute that aggregate (or expose a query that lets the frontend do it cheaply).

import { useEffect, useState, type FormEvent } from 'react';
import type { Config, ReleaseChannel, RsiCookieStatus } from '../api';
import { api, RELEASE_CHANNEL_LABELS } from '../api';
import {
  applyUpdate,
  checkForUpdate,
  type UpdateCheckResult,
  type UpdateInfo,
} from '../updater';
import {
  Field,
  GhostButton,
  PrimaryButton,
  StatusDot,
  TextInput,
  TrayCard,
} from './tray/primitives';
import { ReingestCard } from './ReingestCard';
import { ReparseCard } from './ReparseCard';

interface Props {
  config: Config;
  onSave: (next: Config) => Promise<void>;
}

/**
 * Configuration UI for the tray client.
 *
 * Keeps form state local (uncontrolled-ish) until the user hits Save.
 * No optimistic mutation — the parent re-fetches after save lands so
 * we never display a value that the backend hasn't actually persisted.
 */
export function SettingsPane({ config, onSave }: Props) {
  const [draft, setDraft] = useState<Config>(config);
  const [saving, setSaving] = useState(false);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [pairingCode, setPairingCode] = useState('');
  const [pairing, setPairing] = useState(false);
  const [pairError, setPairError] = useState<string | null>(null);
  const [pairedAs, setPairedAs] = useState<string | null>(null);

  const [cookieStatus, setCookieStatus] = useState<RsiCookieStatus | null>(
    null,
  );
  const [cookieDraft, setCookieDraft] = useState('');
  const [cookieSaving, setCookieSaving] = useState(false);
  const [cookieError, setCookieError] = useState<string | null>(null);
  const [cookieSavedAt, setCookieSavedAt] = useState<number | null>(null);

  // Updates card state. `appVersion` is the Cargo workspace version
  // (e.g. "0.2.0-alpha") sourced via api.getAppVersion() so it
  // matches the GitHub release tag. Tauri's own getVersion() would
  // return the numeric tauri.conf.json value (MSI-friendly subset).
  // `updateState` drives the status
  // text/buttons; `installProgress` is non-null only while a download
  // is in flight.
  const [appVersion, setAppVersion] = useState<string | null>(null);
  const [updateState, setUpdateState] = useState<
    | { kind: 'idle' }
    | { kind: 'checking' }
    | { kind: 'available'; info: UpdateInfo }
    | { kind: 'up_to_date' }
    | { kind: 'error'; message: string }
    | { kind: 'installing' }
  >({ kind: 'idle' });
  const [installProgress, setInstallProgress] = useState<{
    downloaded: number;
    total: number | null;
  } | null>(null);

  useEffect(() => {
    let cancelled = false;
    api
      .getRsiCookieStatus()
      .then((next) => {
        if (!cancelled) setCookieStatus(next);
      })
      .catch((e) => {
        if (!cancelled) setCookieError(String(e));
      });
    api
      .getAppVersion()
      .then((v) => {
        if (!cancelled) setAppVersion(v);
      })
      .catch(() => {
        // Version is informational; if it fails we just don't show it.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Manual "Check for updates" handler. Bypasses the auto-check
  // preference — pressing the button always checks, regardless of the
  // toggle state, because that's the user's explicit intent.
  const handleCheckForUpdate = async () => {
    setUpdateState({ kind: 'checking' });
    try {
      const result: UpdateCheckResult = await checkForUpdate(
        draft.release_channel,
      );
      if (result.available) {
        setUpdateState({ kind: 'available', info: result });
      } else {
        setUpdateState({ kind: 'up_to_date' });
      }
    } catch (e) {
      setUpdateState({ kind: 'error', message: String(e) });
    }
  };

  // "Install and restart". The Rust install command re-checks the
  // channel before downloading, so a fresher release between
  // user-pressed-Install and now would simply install the newer
  // one — fine for our scale, and removes the need to plumb the
  // (non-Serializable) Update handle across the IPC bridge.
  const handleInstallUpdate = async () => {
    if (updateState.kind !== 'available') return;
    setUpdateState({ kind: 'installing' });
    setInstallProgress({ downloaded: 0, total: null });
    try {
      await applyUpdate(draft.release_channel, (downloaded, total) => {
        setInstallProgress({ downloaded, total });
      });
      // applyUpdate calls relaunch(); we never reach this line.
    } catch (e) {
      setInstallProgress(null);
      setUpdateState({ kind: 'error', message: String(e) });
    }
  };

  // Wraps `setDraft` so any in-pane edit clears the trailing "✓ Saved"
  // pip and any save error — both are stale the moment the user
  // resumes editing.
  const editDraft = (mutate: (prev: Config) => Config) => {
    setDraft(mutate);
    setSavedAt(null);
    setError(null);
  };

  const updateRemote = (patch: Partial<Config['remote_sync']>) =>
    editDraft((prev) => ({
      ...prev,
      remote_sync: { ...prev.remote_sync, ...patch },
    }));

  const handlePair = async () => {
    if (!draft.remote_sync.api_url) {
      setPairError('Set the API URL above first.');
      return;
    }
    setPairing(true);
    setPairError(null);
    try {
      const outcome = await api.pairDevice(
        draft.remote_sync.api_url,
        pairingCode,
      );
      // Reload from disk — the Rust side already persisted token +
      // claimed_handle, no point trusting our in-memory draft.
      const fresh = await api.getConfig();
      setDraft(fresh);
      setPairingCode('');
      setPairedAs(outcome.claimed_handle);
    } catch (err) {
      setPairError(String(err));
    } finally {
      setPairing(false);
    }
  };

  const handleSaveCookie = async () => {
    if (!cookieDraft.trim()) {
      setCookieError('Paste the cookie value first.');
      return;
    }
    setCookieSaving(true);
    setCookieError(null);
    try {
      const next = await api.setRsiCookie(cookieDraft.trim());
      setCookieStatus(next);
      setCookieDraft('');
      setCookieSavedAt(Date.now());
    } catch (err) {
      setCookieError(String(err));
    } finally {
      setCookieSaving(false);
    }
  };

  const handleClearCookie = async () => {
    if (
      !window.confirm(
        'Clear the stored RSI cookie? Hangar refresh will pause until you paste a new one.',
      )
    ) {
      return;
    }
    setCookieSaving(true);
    setCookieError(null);
    try {
      const next = await api.clearRsiCookie();
      setCookieStatus(next);
      setCookieSavedAt(null);
    } catch (err) {
      setCookieError(String(err));
    } finally {
      setCookieSaving(false);
    }
  };

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setSaving(true);
    setError(null);
    try {
      await onSave(draft);
      setSavedAt(Date.now());
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  };

  const isPaired = Boolean(
    draft.remote_sync.access_token && draft.remote_sync.claimed_handle,
  );

  return (
    <form
      onSubmit={handleSubmit}
      style={{ display: 'flex', flexDirection: 'column', gap: 12 }}
    >
      <TrayCard title="Game.log">
        <Field
          label="Override path"
          hint="Leave blank to auto-discover the largest LIVE/PTU/EPTU log."
        >
          <TextInput
            type="text"
            value={draft.gamelog_path ?? ''}
            placeholder="auto-discover"
            onChange={(e) =>
              editDraft((prev) => ({
                ...prev,
                gamelog_path: e.target.value || null,
              }))
            }
            spellCheck={false}
          />
        </Field>
      </TrayCard>

      <ReingestCard />

      <ReparseCard />

      <TrayCard
        title="Updates"
        kicker={appVersion ? `v${appVersion}` : undefined}
        right={
          <label
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 6,
              fontSize: 11,
              color: 'var(--fg-muted)',
              cursor: 'pointer',
            }}
            title="Check for updates automatically when the app launches"
          >
            <input
              type="checkbox"
              checked={draft.auto_update_check}
              onChange={(e) =>
                editDraft((prev) => ({
                  ...prev,
                  auto_update_check: e.target.checked,
                }))
              }
              style={{ accentColor: 'var(--accent)' }}
            />
            <span style={{ textTransform: 'uppercase', letterSpacing: '0.1em' }}>
              auto
            </span>
          </label>
        }
      >
        <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
          <UpdateStatusLine state={updateState} progress={installProgress} />
          <label
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 8,
              fontSize: 12,
              color: 'var(--fg-muted)',
            }}
            title="Switch channels at any time. The next check polls the new channel's manifest."
          >
            <span style={{ minWidth: 64 }}>Channel</span>
            <select
              value={draft.release_channel}
              onChange={(e) =>
                editDraft((prev) => ({
                  ...prev,
                  release_channel: e.target.value as ReleaseChannel,
                }))
              }
              disabled={
                updateState.kind === 'checking' ||
                updateState.kind === 'installing'
              }
              style={{
                background: 'var(--bg-2)',
                color: 'var(--fg)',
                border: '1px solid var(--border)',
                borderRadius: 4,
                padding: '4px 6px',
                fontSize: 12,
                fontFamily: 'inherit',
              }}
            >
              {(Object.keys(RELEASE_CHANNEL_LABELS) as ReleaseChannel[]).map(
                (ch) => (
                  <option key={ch} value={ch}>
                    {RELEASE_CHANNEL_LABELS[ch]}
                  </option>
                ),
              )}
            </select>
          </label>
          {updateState.kind === 'available' && updateState.info.notes && (
            <pre
              style={{
                margin: 0,
                padding: '8px 10px',
                background: 'var(--bg-2)',
                border: '1px solid var(--border)',
                borderRadius: 4,
                fontSize: 11,
                lineHeight: 1.5,
                whiteSpace: 'pre-wrap',
                wordBreak: 'break-word',
                maxHeight: 160,
                overflowY: 'auto',
                color: 'var(--fg-muted)',
              }}
            >
              {updateState.info.notes}
            </pre>
          )}
          <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
            {updateState.kind === 'available' ? (
              <PrimaryButton
                type="button"
                onClick={handleInstallUpdate}
                disabled={false}
              >
                Install v{updateState.info.version} and restart
              </PrimaryButton>
            ) : (
              <GhostButton
                type="button"
                onClick={handleCheckForUpdate}
                disabled={
                  updateState.kind === 'checking' ||
                  updateState.kind === 'installing'
                }
              >
                {updateState.kind === 'checking'
                  ? 'Checking…'
                  : 'Check for updates'}
              </GhostButton>
            )}
          </div>
          <label
            style={{
              display: 'flex',
              alignItems: 'flex-start',
              gap: 8,
              fontSize: 12,
              color: 'var(--fg-muted)',
              cursor: 'pointer',
              marginTop: 4,
              borderTop: '1px solid var(--border)',
              paddingTop: 10,
            }}
          >
            <input
              type="checkbox"
              checked={draft.debug_logging}
              onChange={(e) =>
                editDraft((prev) => ({
                  ...prev,
                  debug_logging: e.target.checked,
                }))
              }
              style={{ accentColor: 'var(--accent)', marginTop: 2 }}
            />
            <span style={{ lineHeight: 1.4 }}>
              <strong style={{ color: 'var(--fg)' }}>Debug logging</strong>
              <span style={{ display: 'block', fontSize: 11 }}>
                Writes a daily client.log to the user data dir for bug
                reports. Off by default. Restart after toggling.
              </span>
            </span>
          </label>
        </div>
      </TrayCard>

      <TrayCard
        title="Remote sync"
        right={
          <label
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 6,
              fontSize: 11,
              color: 'var(--fg-muted)',
              cursor: 'pointer',
            }}
          >
            <input
              type="checkbox"
              checked={draft.remote_sync.enabled}
              onChange={(e) => updateRemote({ enabled: e.target.checked })}
              style={{ accentColor: 'var(--accent)' }}
            />
            <span
              style={{ textTransform: 'uppercase', letterSpacing: '0.1em' }}
            >
              {draft.remote_sync.enabled ? 'ON' : 'OFF'}
            </span>
          </label>
        }
      >
        <p
          style={{
            margin: '0 0 12px',
            color: 'var(--fg-muted)',
            fontSize: 12,
            lineHeight: 1.5,
          }}
        >
          Push events to a StarStats API server. Disabled by default — you
          choose when to share.
        </p>

        <fieldset
          disabled={!draft.remote_sync.enabled}
          style={{
            border: 'none',
            margin: 0,
            padding: 0,
            opacity: draft.remote_sync.enabled ? 1 : 0.45,
            display: 'flex',
            flexDirection: 'column',
            gap: 12,
          }}
        >
          <Field label="API URL">
            <TextInput
              type="url"
              value={draft.remote_sync.api_url ?? ''}
              placeholder="https://api.example.com"
              onChange={(e) =>
                updateRemote({ api_url: e.target.value || null })
              }
              spellCheck={false}
            />
          </Field>

          <Field label="Hangar">
            {isPaired ? (
              <div
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'space-between',
                  gap: 12,
                  padding: '8px 10px',
                  background: 'var(--surface-2)',
                  border: '1px solid var(--border)',
                  borderRadius: 'var(--r-sm)',
                }}
              >
                <div
                  style={{ display: 'flex', alignItems: 'center', gap: 10 }}
                >
                  <StatusDot tone="ok" />
                  <div>
                    <div style={{ fontSize: 12, color: 'var(--fg)' }}>
                      Paired as{' '}
                      <strong style={{ color: 'var(--accent)' }}>
                        {draft.remote_sync.claimed_handle}
                      </strong>
                    </div>
                    <div
                      style={{
                        fontSize: 11,
                        color: 'var(--fg-dim)',
                        fontFamily: 'var(--font-mono)',
                      }}
                    >
                      tok_•••• {draft.remote_sync.access_token!.slice(-4)}
                    </div>
                  </div>
                </div>
                <GhostButton
                  type="button"
                  onClick={() => {
                    // Clear both the persisted credentials and any stale
                    // form state — otherwise the previous success/error
                    // message hangs around when the pairing input
                    // re-renders.
                    updateRemote({
                      access_token: null,
                      claimed_handle: null,
                    });
                    setPairingCode('');
                    setPairError(null);
                    setPairedAs(null);
                  }}
                >
                  Unpair
                </GhostButton>
              </div>
            ) : (
              <div
                style={{ display: 'flex', flexDirection: 'column', gap: 8 }}
              >
                <small
                  style={{
                    fontSize: 11,
                    color: 'var(--fg-dim)',
                    lineHeight: 1.4,
                  }}
                >
                  Generate a pairing code on the StarStats website (Hangar
                  → Pair a desktop client) and type it below.
                </small>
                <div style={{ display: 'flex', gap: 8 }}>
                  <TextInput
                    type="text"
                    value={pairingCode}
                    placeholder="ABCDEFGH"
                    maxLength={8}
                    onChange={(e) =>
                      setPairingCode(e.target.value.toUpperCase())
                    }
                    spellCheck={false}
                    autoComplete="off"
                    style={{
                      flex: 1,
                      letterSpacing: '0.25em',
                      textAlign: 'center',
                      fontWeight: 600,
                      fontSize: 14,
                    }}
                  />
                  <PrimaryButton
                    type="button"
                    onClick={handlePair}
                    disabled={pairing || pairingCode.length !== 8}
                  >
                    {pairing ? 'Pairing…' : 'Pair'}
                  </PrimaryButton>
                </div>
                {pairError && (
                  <small style={{ fontSize: 12, color: 'var(--danger)' }}>
                    {pairError}
                  </small>
                )}
                {pairedAs && (
                  <small style={{ fontSize: 12, color: 'var(--ok)' }}>
                    ✓ Paired as {pairedAs}
                  </small>
                )}
              </div>
            )}
          </Field>

          <div
            style={{
              display: 'grid',
              gridTemplateColumns: '1fr 1fr',
              gap: 10,
            }}
          >
            <Field label="Sync interval">
              <div
                style={{ display: 'flex', alignItems: 'center', gap: 6 }}
              >
                <TextInput
                  type="number"
                  min={5}
                  max={3600}
                  value={draft.remote_sync.interval_secs}
                  onChange={(e) =>
                    updateRemote({
                      interval_secs: Math.max(
                        5,
                        Number(e.target.value) || 60,
                      ),
                    })
                  }
                  style={{ flex: 1 }}
                />
                <span style={{ fontSize: 11, color: 'var(--fg-dim)' }}>
                  sec
                </span>
              </div>
            </Field>
            <Field label="Batch size">
              <TextInput
                type="number"
                min={1}
                max={5000}
                value={draft.remote_sync.batch_size}
                onChange={(e) =>
                  updateRemote({
                    batch_size: Math.max(1, Number(e.target.value) || 200),
                  })
                }
              />
            </Field>
          </div>
        </fieldset>
      </TrayCard>

      <TrayCard
        title="RSI session cookie"
        right={
          <span
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 6,
              fontSize: 11,
            }}
          >
            <StatusDot tone={cookieStatus?.configured ? 'ok' : 'warn'} />
            <span
              style={{
                color: cookieStatus?.configured
                  ? 'var(--ok)'
                  : 'var(--warn)',
                fontFamily: 'var(--font-mono)',
                textTransform: 'uppercase',
                letterSpacing: '0.08em',
              }}
            >
              {cookieStatus?.configured ? 'SET' : 'MISSING'}
            </span>
          </span>
        }
      >
        <p
          style={{
            margin: '0 0 12px',
            color: 'var(--fg-muted)',
            fontSize: 12,
            lineHeight: 1.5,
          }}
        >
          {cookieStatus === null ? (
            'Loading…'
          ) : cookieStatus.configured ? (
            <>
              Configured (last 4 chars:{' '}
              <code
                style={{
                  color: 'var(--accent)',
                  fontFamily: 'var(--font-mono)',
                }}
              >
                {cookieStatus.preview ?? '????'}
              </code>
              ). Paste a new value to rotate.
            </>
          ) : (
            'Not configured. Paste your Rsi-Token cookie below.'
          )}
        </p>

        <Field
          label="Rsi-Token cookie"
          hint="Find this in DevTools → Application → Cookies → robertsspaceindustries.com → Rsi-Token. Never leaves your machine — only parsed ship lists are sent."
        >
          <TextInput
            type="password"
            value={cookieDraft}
            placeholder="•••••••••••••••••••••••••••"
            onChange={(e) => {
              setCookieDraft(e.target.value);
              setCookieSavedAt(null);
              setCookieError(null);
            }}
            spellCheck={false}
            autoComplete="off"
          />
        </Field>

        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            marginTop: 10,
          }}
        >
          <PrimaryButton
            type="button"
            onClick={handleSaveCookie}
            disabled={cookieSaving || !cookieDraft.trim()}
          >
            {cookieSaving ? 'Saving…' : 'Save cookie'}
          </PrimaryButton>
          <GhostButton
            type="button"
            onClick={handleClearCookie}
            disabled={cookieSaving || !cookieStatus?.configured}
          >
            Clear
          </GhostButton>
          {cookieSavedAt && !cookieSaving && !cookieError && (
            <span style={{ fontSize: 11, color: 'var(--ok)' }}>✓ Saved</span>
          )}
        </div>
        {cookieError && (
          <small
            style={{
              display: 'block',
              marginTop: 6,
              fontSize: 12,
              color: 'var(--danger)',
            }}
          >
            {cookieError}
          </small>
        )}
      </TrayCard>

      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 12,
          padding: '10px 0',
          borderTop: '1px solid var(--border)',
        }}
      >
        <PrimaryButton type="submit" disabled={saving}>
          {saving ? 'Saving…' : 'Save settings'}
        </PrimaryButton>
        <span style={{ fontSize: 11, color: 'var(--fg-dim)' }}>
          Changes apply on save. Sync state refetches automatically.
        </span>
        {savedAt && !saving && !error && (
          <span style={{ fontSize: 11, color: 'var(--ok)' }}>✓ Saved</span>
        )}
        {error && (
          <span style={{ fontSize: 11, color: 'var(--danger)' }}>{error}</span>
        )}
      </div>
    </form>
  );
}

/**
 * Single-line status string + optional download progress for the
 * Updates card. Kept as a separate component so the Updates JSX
 * stays readable; it doesn't need any of SettingsPane's state.
 */
function UpdateStatusLine({
  state,
  progress,
}: {
  state:
    | { kind: 'idle' }
    | { kind: 'checking' }
    | { kind: 'available'; info: UpdateInfo }
    | { kind: 'up_to_date' }
    | { kind: 'error'; message: string }
    | { kind: 'installing' };
  progress: { downloaded: number; total: number | null } | null;
}) {
  const baseStyle = {
    margin: 0,
    fontSize: 12,
    lineHeight: 1.5,
  } as const;
  switch (state.kind) {
    case 'idle':
      return (
        <p style={{ ...baseStyle, color: 'var(--fg-muted)' }}>
          Click "Check for updates" to query GitHub releases.
        </p>
      );
    case 'checking':
      return (
        <p style={{ ...baseStyle, color: 'var(--fg-muted)' }}>
          Checking for updates…
        </p>
      );
    case 'up_to_date':
      return (
        <p style={{ ...baseStyle, color: 'var(--ok)' }}>
          You're on the latest version.
        </p>
      );
    case 'available':
      return (
        <p style={{ ...baseStyle, color: 'var(--accent)' }}>
          Update available: <strong>v{state.info.version}</strong>
          {state.info.date && (
            <span style={{ color: 'var(--fg-dim)', fontSize: 11 }}>
              {' · '}
              {state.info.date}
            </span>
          )}
        </p>
      );
    case 'installing': {
      const pct =
        progress && progress.total && progress.total > 0
          ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100))
          : null;
      return (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
          <p style={{ ...baseStyle, color: 'var(--accent)' }}>
            {pct !== null
              ? `Downloading… ${pct}%`
              : 'Downloading…'}
          </p>
          {progress && (
            <progress
              max={progress.total ?? undefined}
              value={progress.downloaded}
              style={{ width: '100%', height: 6 }}
            />
          )}
        </div>
      );
    }
    case 'error':
      return (
        <p style={{ ...baseStyle, color: 'var(--danger)' }}>
          {state.message}
        </p>
      );
  }
}

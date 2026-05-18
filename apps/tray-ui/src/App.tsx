import { useCallback, useEffect, useState } from 'react';
import { api, type Config, type SettingsField } from './api';
import { StatusPane } from './components/StatusPane';
import { SettingsPane } from './components/SettingsPane';
import { LogsPane } from './components/LogsPane';
import { TrayHeader, type TrayView } from './components/TrayHeader';
import { SubmissionsPane } from './submissions/SubmissionsPane';
import { useStatusPolling } from './hooks/useStatusPolling';
import { FieldFocusProvider, useFieldFocus } from './hooks/useFieldFocus';
import './styles.css';

/** How often the unknown-line badge polls storage. Cheap query
 *  (single indexed COUNT) so 30s is comfortable; tweak down if a
 *  flood of unknowns mid-session feels stale. */
const UNKNOWN_BADGE_REFRESH_MS = 30_000;

function AppInner() {
  const [view, setView] = useState<TrayView>('status');
  const [config, setConfig] = useState<Config | null>(null);
  const [error, setError] = useState<string | null>(null);
  const fieldFocus = useFieldFocus();

  // Apply the persisted theme to the document root. `index.html` ships
  // with `data-theme="stanton"` so the unstyled-flash before config
  // loads is still a valid theme; once config arrives we swap the
  // attribute and the four `[data-theme="..."]` token blocks in
  // `starstats-tokens.css` repaint without a reflow.
  useEffect(() => {
    if (config?.theme) {
      document.documentElement.dataset.theme = config.theme;
    }
  }, [config?.theme]);
  // Sourced via the Tauri command (Rust CARGO_PKG_VERSION) rather
  // than a Vite build-time constant from `package.json`, because
  // `package.json` was the wrong source of truth — it shipped at
  // 0.1.0 while the workspace Cargo.toml advanced through several
  // releases. One source of truth (the Rust binary), one fetch.
  const [appVersion, setAppVersion] = useState<string | null>(null);

  const { status, refresh: refreshStatus } = useStatusPolling({
    active: view === 'status',
    onError: setError,
  });

  // Local count of unknown shapes pending review. Drives the badge on
  // the Review tab; SubmissionsPane reports back via `onCountChange`
  // when it refetches after a Submit/Dismiss so we don't double-poll.
  const [unknownCount, setUnknownCount] = useState(0);
  useEffect(() => {
    let cancelled = false;
    const tick = async () => {
      try {
        const n = await api.countUnknownLines();
        if (!cancelled) setUnknownCount(n);
      } catch {
        // Badge is informational — silent fallback keeps the
        // header noise-free if the IPC layer hiccups.
      }
    };
    void tick();
    const id = window.setInterval(tick, UNKNOWN_BADGE_REFRESH_MS);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, []);

  const refreshConfig = useCallback(async () => {
    try {
      const c = await api.getConfig();
      setConfig(c);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refreshConfig();
    let cancelled = false;
    api
      .getAppVersion()
      .then((v) => {
        if (!cancelled) setAppVersion(v);
      })
      .catch(() => {
        // Version is informational; if it fails the header just
        // omits it. Already shown in Settings → Updates with its
        // own error path, so silent fallback is fine here.
      });
    return () => {
      cancelled = true;
    };
  }, [refreshConfig]);

  const onSaveConfig = async (next: Config) => {
    await api.saveConfig(next);
    setConfig(next);
    void refreshStatus();
  };

  // HealthCard CTAs land here: switch the view to Settings and focus
  // the targeted field via the ref registry. SettingsPane registers
  // its field refs in an effect; useFieldFocus.focus retries on
  // animation frames until the ref shows up (bounded).
  const onGoToSettings = useCallback(
    (field: SettingsField) => {
      setView('settings');
      fieldFocus.focus(field);
    },
    [fieldFocus]
  );

  const isTailing = status?.tail.current_path != null;

  return (
    <div className="app">
      <TrayHeader
        view={view}
        onView={setView}
        isTailing={isTailing}
        version={appVersion}
        reviewBadge={unknownCount}
      />
      <main className="app__main">
        {error && <div className="error">Error: {error}</div>}
        {/*
          Keyed wrapper triggers the design system's `.ss-screen-enter`
          fade-and-lift animation every time the user switches tabs;
          inside, the staggered `.ss-card` mount-in animations cascade
          per the nth-child rules in `starstats-tokens.css`.
        */}
        <div key={view} className="ss-screen-enter">
          {view === 'status' &&
            (status ? (
              <StatusPane
                status={status}
                webOrigin={config?.web_origin ?? null}
                onGoToSettings={onGoToSettings}
              />
            ) : (
              <div className="loading">Loading…</div>
            ))}
          {view === 'logs' && <LogsPane />}
          {view === 'review' && (
            <SubmissionsPane onCountChange={setUnknownCount} />
          )}
          {view === 'settings' &&
            (config ? (
              <SettingsPane config={config} onSave={onSaveConfig} />
            ) : (
              <div className="loading">Loading…</div>
            ))}
        </div>
      </main>
    </div>
  );
}

export default function App() {
  return (
    <FieldFocusProvider>
      <AppInner />
    </FieldFocusProvider>
  );
}

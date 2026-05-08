import { useCallback, useEffect, useState } from 'react';
import { api, type Config } from './api';
import { StatusPane } from './components/StatusPane';
import { SettingsPane } from './components/SettingsPane';
import { LogsPane } from './components/LogsPane';
import { TrayHeader, type TrayView } from './components/TrayHeader';
import { useStatusPolling } from './hooks/useStatusPolling';
import './styles.css';

export default function App() {
  const [view, setView] = useState<TrayView>('status');
  const [config, setConfig] = useState<Config | null>(null);
  const [error, setError] = useState<string | null>(null);
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

  const isTailing = status?.tail.current_path != null;

  return (
    <div className="app">
      <TrayHeader
        view={view}
        onView={setView}
        isTailing={isTailing}
        version={appVersion}
      />
      <main className="app__main">
        {error && <div className="error">Error: {error}</div>}
        {view === 'status' &&
          (status ? (
            <StatusPane
              status={status}
              webOrigin={
                config?.web_origin ?? config?.remote_sync.api_url ?? null
              }
              onGoToSettings={() => setView('settings')}
            />
          ) : (
            <div className="loading">Loading…</div>
          ))}
        {view === 'logs' && <LogsPane />}
        {view === 'settings' &&
          (config ? (
            <SettingsPane config={config} onSave={onSaveConfig} />
          ) : (
            <div className="loading">Loading…</div>
          ))}
      </main>
    </div>
  );
}
